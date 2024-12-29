use std::hash::{DefaultHasher, Hash, Hasher};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use sqlx::prelude::FromRow;

use super::i64_as_string;
use crate::db::{bool_to_sqlite, sqlite_to_bool, RawBoolean, RawTimestamp};

pub type RawConfigHash = i64;
pub type ConfigHash = u64;

#[derive(Deserialize, Serialize, FromRow)]
pub struct RawConfig {
    pub hash: RawConfigHash,
    pub source_url: String,
    pub fetch_interval_s: i64,
    pub try_parse_json: RawBoolean,
    pub active: RawBoolean,
    pub last_fetched: Option<RawTimestamp>,
}

impl From<Config> for RawConfig {
    fn from(val: Config) -> Self {
        Self {
            hash: val.hash as RawConfigHash,
            source_url: val.source_url,
            fetch_interval_s: val.fetch_interval_s,
            try_parse_json: bool_to_sqlite(val.try_parse_json),
            active: bool_to_sqlite(val.active),
            last_fetched: val.last_fetched.map(Timestamp::as_second),
        }
    }
}
impl From<RawConfig> for Config {
    fn from(raw: RawConfig) -> Self {
        Self {
            hash: raw.hash as ConfigHash,
            source_url: raw.source_url,
            fetch_interval_s: raw.fetch_interval_s,
            try_parse_json: sqlite_to_bool(raw.try_parse_json),
            active: sqlite_to_bool(raw.active),
            last_fetched: raw.last_fetched.map(|s| Timestamp::new(s, 0).unwrap()),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    #[serde(with = "i64_as_string")]
    pub hash: ConfigHash,
    pub source_url: String,
    pub fetch_interval_s: i64,
    pub try_parse_json: bool,
    pub active: bool,
    pub last_fetched: Option<Timestamp>,
}

impl From<ConfigInput> for Config {
    fn from(inp: ConfigInput) -> Self {
        let mut hasher = DefaultHasher::new();
        inp.hash(&mut hasher);
        let hash = hasher.finish();
        Self {
            hash,
            source_url: inp.source_url,
            fetch_interval_s: inp.fetch_interval_s,
            try_parse_json: inp.try_parse_json,
            last_fetched: None,
            active: true,
        }
    }
}

#[derive(Deserialize, Serialize, Hash)]
pub struct ConfigInput {
    pub source_url: String,
    pub fetch_interval_s: i64,
    pub try_parse_json: bool,
}

#[derive(Deserialize, Serialize)]
pub struct FetcheConfig {
    pub configs: Vec<ConfigInput>,
}
