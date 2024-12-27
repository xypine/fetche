use db::{connect, create_or_activate_config, deactivate_all_configs, get_active_configs, get_config, record_downtime, record_fetch, record_fetch_config};
use jiff::Timestamp;
use models::{config::{Config, ConfigHash, FetcheConfig}, fetch_result::{FetchRecord, Status}};
use sqlx::SqlitePool;
use tokio_cron_scheduler::{Job, JobScheduler};
use tokio::signal;

pub mod models;
pub mod db;

pub struct Query {
    /// Fill gaps between datapoints with the previous data
    /// - fetche being down will be marked as FetcheDown, not as the previous record
    pub decompress: bool
}

#[tokio::main]
async fn main() {
    let pool = connect().await;
    sqlx::migrate!().run(&pool).await.expect("Failed to run migrations");
    deactivate_all_configs(&pool).await.unwrap();
    println!("Reading config...");
    let config_path = std::env::var("FETCHE_CONFIG_PATH").unwrap_or("./fetche.toml".to_owned());
    let config_str = std::fs::read_to_string(config_path).expect("Failed to read config, make sure ./fetche.toml exists or that FETCHE_CONFIG_PATH points to the right location");
    let parsed_config: FetcheConfig = toml::from_str(&config_str).expect("Failed to parse fetche.toml");
    let mut activation_handles = vec![];
    for input in parsed_config.configs {
        let client = pool.clone();
        let handle = tokio::spawn(async move {
            let config = Config::from(input);
            println!("Activating config: {config:#?}");
            create_or_activate_config(&client, config.clone()).await.expect("Failed to activate config");
            tick(&client, config.hash).await.unwrap();
        });
        activation_handles.push(handle);
    }
    for handle in activation_handles {
        handle.await.expect("Panic in task");
    }

    let mut sched = JobScheduler::new().await.expect("Failed to create a job scheduler");
    let pool = connect().await;
    sched.add(
        Job::new_async("0/1 * * * * *", move |_uuid, _l| {
            let pool = pool.clone();
            Box::pin(async move {
                let configs = get_active_configs(&pool).await.expect("Failed to fetch active configs");
                let mut tick_handles = vec![];
                for config in configs {
                    let client = pool.clone();
                    let handle = tokio::spawn(async move {
                        return tick(&client, config).await.unwrap();
                    });
                    tick_handles.push(handle);
                }
                for handle in tick_handles {
                    let elapsed_opt = handle.await.expect("Panic in task");
                    if let Some(elapsed) = elapsed_opt {
                        print!("{elapsed}");
                    }
                    print!("\t");
                }
                println!();
            })
        }).expect("Failed to create job")
    ).await.expect("Failed to add job to scheduler");

    sched.shutdown_on_ctrl_c();
    sched.set_shutdown_handler(Box::new(|| {
        Box::pin(async move {
            println!("Shut down done");
        })
    }));

    sched.start().await.expect("Failed to start scheduler");
    signal::ctrl_c().await.unwrap();
}

async fn tick(db: &SqlitePool, config_hash: ConfigHash) -> Result<Option<i64>, sqlx::Error> {
    let config = get_config(db, config_hash).await?;
    let mut since = None;
    if let Some(config) = config {
        let time_since_last_fetch = record_downtime(db, &config).await?;

        let should_fetch = match time_since_last_fetch {
            None => true,
            Some(elapsed) => {
                since = Some(elapsed);
                elapsed >= config.fetch_interval_s
            }
        };

        if should_fetch {
            let fetched_at = Timestamp::now();
            let source_url = config.source_url.clone();
            let fetch_result = reqwest::get(&source_url).await;

            let created_at = Timestamp::now();

            let result: FetchRecord = match fetch_result {
                Ok(resp) => {
                    let rstatus = resp.status();
                    let statuscode = rstatus.as_u16();
                    let status = if rstatus.is_success() {
                        Status::HttpOk(statuscode)
                    } else {
                        Status::HttpErr(statuscode)
                    };
                    let body_text = resp.text().await.ok();
                    let valid_json = match (config.try_parse_json, &body_text) {
                        (true, Some(text)) => {
                            Some(
                                match serde_json::from_str::<serde_json::Value>(text) {
                                    Ok(_) => true,
                                    _ => false
                                }
                            )
                        },
                        _ => None
                    };
                    FetchRecord {
                        config: config_hash,
                        fetched_at,
                        created_at,
                        source_url,
                        body_text,
                        valid_json,
                        status,
                        from_db: true,
                    }
                },
                Err(_) => FetchRecord {
                    config: config_hash,
                    fetched_at,
                    created_at,
                    source_url,
                    body_text: None,
                    valid_json: None,
                    status: Status::Error,
                    from_db: true,
                },
            };
            let _diff = record_fetch(db, result).await?;
            //println!("[{config_hash}] Seconds since last update: {diff:?}");
            record_fetch_config(db, config).await?;
        }
    }

    return Ok(since)
}
