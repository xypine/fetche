use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use sqlx::prelude::FromRow;

use crate::db::RawTimestamp;

use super::config::{ConfigHash, RawConfigHash};

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(tag = "tag", content = "code")]
pub enum Status {
    /// The source returned an ok http status
    HttpOk(u16),
    /// The source returned a non-ok http status
    HttpErr(u16),
    /// No response, probably because fetche or the service was disconnected
    Error,
    /// Fetche was not up at this time - no request was made
    Unknown
}

#[derive(Deserialize, Serialize, FromRow)]
pub struct RawFetchRecord {
    pub config: RawConfigHash,
    pub fetched_at: RawTimestamp,
    pub created_at: RawTimestamp,
    pub source_url: String,
    pub status: String,
    pub body_text: Option<String>,
    pub valid_json: Option<bool>,
}

impl From<FetchRecord> for RawFetchRecord {
    fn from(val: FetchRecord) -> Self {
        let status = serde_json::to_string(&val.status).unwrap();
        Self {
            config: val.config as RawConfigHash,
            fetched_at: val.fetched_at.as_second(),
            created_at: val.created_at.as_second(),
            source_url: val.source_url,
            status,
            body_text: val.body_text,
            valid_json: val.valid_json,
        }
    }
}
impl TryFrom<RawFetchRecord> for FetchRecord {
    type Error = ();

    fn try_from(raw: RawFetchRecord) -> Result<Self, Self::Error> {
        let status: Status = serde_json::from_str(&raw.status).map_err(|_| ())?;

        Ok(
            Self {
                config: raw.config as ConfigHash,
                fetched_at: Timestamp::new(raw.fetched_at, 0).map_err(|_| ())?,
                created_at: Timestamp::new(raw.created_at, 0).map_err(|_| ())?,
                source_url: raw.source_url,
                status,
                body_text: raw.body_text,
                valid_json: raw.valid_json,
                from_db: true,
            }
        )
    }
}

#[derive(Deserialize, Serialize)]
pub struct FetchRecord {
    pub config: ConfigHash,
    pub fetched_at: Timestamp,
    pub created_at: Timestamp,
    pub source_url: String,
    pub status: Status,
    pub body_text: Option<String>,
    pub valid_json: Option<bool>,
    /// actual record or a result of decompression?
    pub from_db: bool,
}

impl PartialEq for FetchRecord {
    fn eq(&self, other: &Self) -> bool {
        return 
            self.config == other.config
            && self.source_url == other.source_url
            && self.status == other.status
            && self.body_text == other.body_text
            && self.valid_json == other.valid_json
            && self.from_db == other.from_db
        ;
    }
}
