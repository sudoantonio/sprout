use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sprout_api_contract::CollectionPageQuery;
use uuid::Uuid;

use crate::error::AppError;

const DEFAULT_LIMIT: u16 = 50;
const MAX_LIMIT: u16 = 100;

#[derive(Clone, Copy)]
pub(super) struct CollectionPage {
    pub after_created_at: Option<DateTime<Utc>>,
    pub after_id: Option<Uuid>,
    pub limit: usize,
}

impl CollectionPage {
    pub fn sql_limit(self) -> Result<i64, AppError> {
        i64::try_from(self.limit + 1).map_err(|_| AppError::Internal)
    }
}

#[derive(Deserialize, Serialize)]
struct CursorPayload {
    version: u8,
    created_at: DateTime<Utc>,
    id: Uuid,
}

pub(super) fn parse_page(query: CollectionPageQuery) -> Result<CollectionPage, AppError> {
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(AppError::BadRequest("collection limit is out of range"));
    }
    let cursor = query.cursor.as_deref().map(decode_cursor).transpose()?;
    Ok(CollectionPage {
        after_created_at: cursor.as_ref().map(|cursor| cursor.created_at),
        after_id: cursor.map(|cursor| cursor.id),
        limit: usize::from(limit),
    })
}

pub(super) fn finish_page<T>(
    rows: &mut Vec<T>,
    page: CollectionPage,
    key: impl FnOnce(&T) -> (DateTime<Utc>, Uuid),
) -> Result<Option<String>, AppError> {
    if rows.len() <= page.limit {
        return Ok(None);
    }
    rows.truncate(page.limit);
    let (created_at, id) = key(rows.last().ok_or(AppError::Internal)?);
    encode_cursor(CursorPayload {
        version: 1,
        created_at,
        id,
    })
}

fn encode_cursor(cursor: CursorPayload) -> Result<Option<String>, AppError> {
    let encoded = serde_json::to_vec(&cursor).map_err(|_| AppError::Internal)?;
    Ok(Some(URL_SAFE_NO_PAD.encode(encoded)))
}

fn decode_cursor(value: &str) -> Result<CursorPayload, AppError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| AppError::BadRequest("invalid collection cursor"))?;
    let cursor: CursorPayload = serde_json::from_slice(&bytes)
        .map_err(|_| AppError::BadRequest("invalid collection cursor"))?;
    if cursor.version != 1 {
        return Err(AppError::BadRequest("unsupported collection cursor"));
    }
    Ok(cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_cursor_round_trips_and_rejects_invalid_input() {
        let created_at = Utc::now();
        let id = Uuid::new_v4();
        let cursor = encode_cursor(CursorPayload {
            version: 1,
            created_at,
            id,
        })
        .unwrap()
        .unwrap();
        let decoded = decode_cursor(&cursor).unwrap();
        assert_eq!(decoded.created_at, created_at);
        assert_eq!(decoded.id, id);
        assert!(decode_cursor("not-a-cursor").is_err());
    }

    #[test]
    fn collection_limits_are_bounded() {
        assert_eq!(
            parse_page(CollectionPageQuery::default()).unwrap().limit,
            usize::from(DEFAULT_LIMIT)
        );
        for limit in [0, MAX_LIMIT + 1] {
            assert!(
                parse_page(CollectionPageQuery {
                    cursor: None,
                    limit: Some(limit),
                })
                .is_err()
            );
        }
    }
}
