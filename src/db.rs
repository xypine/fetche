use jiff::Timestamp;
use sqlx::SqlitePool;

use crate::models::{
    config::{Config, ConfigHash, RawConfig, RawConfigHash},
    fetch_result::{FetchRecord, RawFetchRecord, Status},
};

pub type DBConn = SqlitePool;
pub type RawTimestamp = i64;

pub type RawBoolean = i64;
pub fn bool_to_sqlite(bool: bool) -> RawBoolean {
    match bool {
        true => 1,
        false => 0,
    }
}
pub fn sqlite_to_bool(int: RawBoolean) -> bool {
    match int {
        1 => true,
        _ => false,
    }
}

pub async fn connect() -> DBConn {
    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set!");
    let pool = SqlitePool::connect(&db_url)
        .await
        .expect("Failed to init db connection pool");
    pool
}

pub async fn deactivate_all_configs(db: &DBConn) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE "config" SET active = false
    "#
    )
    .execute(db)
    .await?;
    Ok(())
}

pub async fn create_or_activate_config(db: &DBConn, config: Config) -> Result<(), sqlx::Error> {
    let raw = RawConfig::from(config);
    sqlx::query!(
        r#"
            INSERT INTO "config"
                (hash, source_url, fetch_interval_s, try_parse_json, active)
            VALUES
                ($1, $2, $3, $4, $5)
            ON CONFLICT(hash) DO UPDATE 
                SET active = $5
        "#,
        raw.hash,
        raw.source_url,
        raw.fetch_interval_s,
        raw.try_parse_json,
        raw.active
    )
    .execute(db)
    .await?;
    Ok(())
}

pub async fn get_active_configs(db: &DBConn) -> Result<Vec<ConfigHash>, sqlx::Error> {
    let raw = sqlx::query_scalar!(
        r#"
        SELECT hash FROM "config"
        WHERE active = TRUE
    "#
    )
    .fetch_all(db)
    .await?;
    Ok(raw.into_iter().map(|hash| hash as ConfigHash).collect())
}

pub async fn get_config(
    db: &DBConn,
    config_hash: ConfigHash,
) -> Result<Option<Config>, sqlx::Error> {
    let rawhash = config_hash as RawConfigHash;
    let res = sqlx::query_as!(
        RawConfig,
        r#"
       SELECT * FROM "config" WHERE hash = $1
    "#,
        rawhash
    )
    .fetch_optional(db)
    .await?;
    Ok(res.map(Config::from))
}

pub async fn record_downtime(db: &DBConn, config: &Config) -> Result<Option<i64>, sqlx::Error> {
    let now = Timestamp::now().as_second();
    let mut since_last_fetch = None;

    if let Some(last_fetched) = config.last_fetched {
        let time_since_last_fetch = now - last_fetched.as_second();
        since_last_fetch = Some(time_since_last_fetch);
        //println!("[{}] Seconds since last fetch: {time_since_last_fetch}", config.hash);
        if time_since_last_fetch > (config.fetch_interval_s + 1) {
            //println!("[{} WARN] downtime detected!", config.hash);
            record_fetch(
                db,
                FetchRecord {
                    config: config.hash,
                    fetched_at: Timestamp::new(
                        last_fetched.as_second() + config.fetch_interval_s,
                        0,
                    )
                    .unwrap(),
                    created_at: Timestamp::now(),
                    source_url: config.source_url.clone(),
                    status: Status::Unknown,
                    body_text: None,
                    valid_json: None,
                    from_db: true,
                },
            )
            .await?;
        }
    }

    Ok(since_last_fetch)
}

pub async fn record_fetch_config(db: &DBConn, config: Config) -> Result<(), sqlx::Error> {
    let raw = RawConfig::from(config);

    let now = Timestamp::now().as_second();
    sqlx::query!(
        r#"UPDATE "config" SET last_fetched = $2 WHERE hash = $1"#,
        raw.hash,
        now
    )
    .execute(db)
    .await?;

    Ok(())
}

pub async fn record_fetch(db: &DBConn, fetch: FetchRecord) -> Result<Option<i64>, sqlx::Error> {
    let db_config_hash = fetch.config as RawConfigHash;
    let latest_result = sqlx::query_as!(
        RawFetchRecord,
        r#"
        SELECT * FROM "fetch_result"
        WHERE "config" = $1
        ORDER BY fetched_at DESC
        LIMIT 1
    "#,
        db_config_hash
    )
    .fetch_optional(db)
    .await?
    .map(FetchRecord::try_from)
    .map(|parse_r| parse_r.unwrap());

    let mut skip = false;
    let mut diff = None;
    if let Some(latest) = latest_result {
        diff = Some(fetch.fetched_at.as_second() - latest.fetched_at.as_second());
        if latest == fetch {
            //println!("[{}] identical to last result, skipping", fetch.config);
            skip = true;
        }
    }

    if !skip {
        let raw = RawFetchRecord::from(fetch);
        sqlx::query!(
            r#"
                INSERT INTO "fetch_result"
                    (config, fetched_at, created_at, source_url, status, body_text, valid_json)
                VALUES
                    ($1, $2, $3, $4, $5, $6, $7)
            "#,
            raw.config,
            raw.fetched_at,
            raw.created_at,
            raw.source_url,
            raw.status,
            raw.body_text,
            raw.valid_json
        )
        .execute(db)
        .await?;
    }

    Ok(diff)
}
