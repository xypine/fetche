use std::collections::HashMap;

use api::run_server;
use db::{
    connect, create_or_activate_config, deactivate_all_configs, get_active_configs, get_config,
    record_downtime, record_fetch, record_fetch_config,
};
use jiff::Timestamp;
use models::{
    config::{Config, ConfigHash, FetcheConfig, RawConfigHash},
    fetch_result::{FetchRecord, RawFetchRecord, Status},
};
use sqlx::SqlitePool;
use tokio_cron_scheduler::{Job, JobScheduler};

pub mod api;
pub mod db;
pub mod models;

#[derive(Debug, Default, serde::Deserialize)]
pub struct Query {
    /// Fill gaps between datapoints with previous data, if known
    /// - fetche being down will be marked as FetcheDown, not as the previous record
    #[serde(default)]
    pub decompress: bool,
    #[serde(default)]
    pub filter_config: Option<ConfigHash>,
}

#[tokio::main]
async fn main() {
    let pool = connect().await;
    sqlx::migrate!()
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    let _ = run_query(
        &pool,
        Query {
            decompress: true,
            filter_config: None,
        },
    )
    .await;

    deactivate_all_configs(&pool).await.unwrap();

    println!("Reading config...");
    let config_path = std::env::var("FETCHE_CONFIG_PATH").unwrap_or("./fetche.toml".to_owned());
    let config_str = std::fs::read_to_string(config_path).expect("Failed to read config, make sure ./fetche.toml exists or that FETCHE_CONFIG_PATH points to the right location");
    let parsed_config: FetcheConfig =
        toml::from_str(&config_str).expect("Failed to parse fetche.toml");
    let mut activation_handles = vec![];
    for input in parsed_config.configs {
        let client = pool.clone();
        let handle = tokio::spawn(async move {
            let config = Config::from(input);
            println!("Activating config: {config:#?}");
            create_or_activate_config(&client, config.clone())
                .await
                .expect("Failed to activate config");
            tick(&client, config.hash).await.unwrap();
        });
        activation_handles.push(handle);
    }
    for handle in activation_handles {
        handle.await.expect("Panic in task");
    }

    let mut sched = JobScheduler::new()
        .await
        .expect("Failed to create a job scheduler");
    let pool = connect().await;
    sched
        .add(
            Job::new_async("0/1 * * * * *", move |_uuid, _l| {
                let pool = pool.clone();
                Box::pin(async move {
                    let configs = get_active_configs(&pool)
                        .await
                        .expect("Failed to fetch active configs");
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
            })
            .expect("Failed to create job"),
        )
        .await
        .expect("Failed to add job to scheduler");

    sched.shutdown_on_ctrl_c();
    sched.set_shutdown_handler(Box::new(|| {
        Box::pin(async move {
            println!("Shut down done");
        })
    }));

    sched.start().await.expect("Failed to start scheduler");
    run_server().await.unwrap();
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
                            Some(match serde_json::from_str::<serde_json::Value>(text) {
                                Ok(_) => true,
                                _ => false,
                            })
                        }
                        _ => None,
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
                }
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

    return Ok(since);
}

async fn run_query(
    db: &SqlitePool,
    query: Query,
) -> Result<HashMap<ConfigHash, Vec<FetchRecord>>, sqlx::Error> {
    let records = match query.filter_config {
        Some(config) => {
            let db_config_hash = config as RawConfigHash;
            sqlx::query_as!(
                RawFetchRecord,
                r#"
                SELECT * FROM "fetch_result"
                WHERE "config" = $1
                ORDER BY fetched_at ASC
            "#,
                db_config_hash
            )
            .fetch_all(db)
            .await?
            .into_iter()
            .map(FetchRecord::try_from)
            .map(|parse_r| parse_r.unwrap())
            .collect::<Vec<_>>()
        }
        None => sqlx::query_as!(
            RawFetchRecord,
            r#"
                SELECT * FROM "fetch_result"
                ORDER BY fetched_at ASC
            "#
        )
        .fetch_all(db)
        .await?
        .into_iter()
        .map(FetchRecord::try_from)
        .map(|parse_r| parse_r.unwrap())
        .collect::<Vec<_>>(),
    };

    let mut configs = HashMap::new();
    for r in &records {
        if !configs.contains_key(&r.config) {
            let config = get_config(db, r.config)
                .await?
                .expect("No such config - programmer error");
            configs.insert(r.config, config);
        }
    }

    let mut records_by_config = HashMap::new();
    for config in configs.values() {
        println!("[{}]", config.hash);
        let mut config_records = vec![];
        let mut matching_records: Vec<_> = records
            .iter()
            .filter(|r| r.config == config.hash)
            .cloned()
            .collect();

        // No changes detected between last record and now
        // we can fill the gap
        match (
            query.decompress,
            config.last_fetched,
            matching_records.last(),
        ) {
            (true, Some(config_last_fetched), Some(last))
                if config_last_fetched != last.created_at =>
            {
                matching_records.push(FetchRecord {
                    config: last.config,
                    source_url: last.source_url.clone(),
                    status: last.status,
                    fetched_at: config_last_fetched,
                    created_at: Timestamp::now(),
                    body_text: last.body_text.clone(),
                    valid_json: last.valid_json,
                    from_db: false,
                });
            }
            _ => {}
        }

        let mut previous_record: Option<FetchRecord> = None;
        for record in matching_records {
            let fethed_at = record.fetched_at.as_second();
            println!(
                "\tevent at {fethed_at}: {:?} {}",
                record.status, record.source_url
            );

            if query.decompress {
                if let Some(prev) = previous_record {
                    let mut diff = fethed_at - prev.fetched_at.as_second();
                    while diff > (config.fetch_interval_s + 1) {
                        //println!("\t\tfilling diff {diff}");
                        let new_at = fethed_at - diff + config.fetch_interval_s;

                        config_records.push(FetchRecord {
                            config: prev.config,
                            source_url: prev.source_url.clone(),
                            status: prev.status,
                            from_db: false,
                            fetched_at: Timestamp::new(new_at, 0).unwrap(),
                            created_at: Timestamp::now(),
                            body_text: prev.body_text.clone(),
                            valid_json: prev.valid_json,
                        });

                        diff = fethed_at - new_at;
                    }
                }
            }

            previous_record = Some(record.clone());
            config_records.push(record);
        }
        records_by_config.insert(config.hash, config_records);
    }

    Ok(records_by_config)
}
