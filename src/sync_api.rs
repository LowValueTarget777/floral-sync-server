use crate::{
    auth::bearer_token_matches,
    config::RuntimeConfig,
    protocol::{HealthResponse, PushRequest, WaitResponse},
    store::{StoreError, SyncStore},
};
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use std::time::Duration;

const DEFAULT_WAIT_TIMEOUT_SECONDS: u64 = 25;
const MAX_WAIT_TIMEOUT_SECONDS: u64 = 55;

#[derive(Clone)]
pub struct AppState {
    store: SyncStore,
    runtime_config: RuntimeConfig,
}

impl AppState {
    pub fn new(store: SyncStore, runtime_config: RuntimeConfig) -> Self {
        Self {
            store,
            runtime_config,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ChangesQuery {
    since: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitQuery {
    since: Option<u64>,
    timeout_seconds: Option<u64>,
}

pub async fn health(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<HealthResponse>, ApiError> {
    authorize(&headers, &state.runtime_config.effective_config().sync_token)?;
    Ok(Json(HealthResponse {
        ok: true,
        revision: state.store.revision()?,
    }))
}

pub async fn changes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ChangesQuery>,
) -> Result<impl IntoResponse, ApiError> {
    authorize(&headers, &state.runtime_config.effective_config().sync_token)?;
    Ok(Json(state.store.changes_since(query.since.unwrap_or(0))?))
}

pub async fn push(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PushRequest>,
) -> Result<impl IntoResponse, ApiError> {
    authorize(&headers, &state.runtime_config.effective_config().sync_token)?;
    Ok(Json(state.store.push(request)?))
}

pub async fn wait_for_change(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<WaitQuery>,
) -> Result<Json<WaitResponse>, ApiError> {
    authorize(&headers, &state.runtime_config.effective_config().sync_token)?;

    let since = query.since.unwrap_or(0);
    let timeout_seconds = query
        .timeout_seconds
        .unwrap_or(DEFAULT_WAIT_TIMEOUT_SECONDS)
        .clamp(1, MAX_WAIT_TIMEOUT_SECONDS);
    let mut revisions = state.store.subscribe_revisions();
    let current = *revisions.borrow_and_update();
    if current > since {
        return Ok(Json(WaitResponse {
            revision: current,
            changed: true,
        }));
    }

    match tokio::time::timeout(Duration::from_secs(timeout_seconds), revisions.changed()).await {
        Ok(Ok(())) => Ok(Json(WaitResponse {
            revision: *revisions.borrow_and_update(),
            changed: true,
        })),
        Ok(Err(_)) => Err(ApiError::Internal("revision wait channel closed".into())),
        Err(_) => Ok(Json(WaitResponse {
            revision: *revisions.borrow_and_update(),
            changed: false,
        })),
    }
}

fn authorize(headers: &HeaderMap, token: &str) -> Result<(), ApiError> {
    let Some(value) = headers.get(axum::http::header::AUTHORIZATION) else {
        return Err(ApiError::Unauthorized);
    };
    let Ok(value) = value.to_str() else {
        return Err(ApiError::Unauthorized);
    };
    if bearer_token_matches(Some(value), token) {
        Ok(())
    } else {
        Err(ApiError::Unauthorized)
    }
}

#[derive(Debug)]
pub enum ApiError {
    Unauthorized,
    Store(StoreError),
    Internal(String),
}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        match self {
            ApiError::Unauthorized => StatusCode::UNAUTHORIZED.into_response(),
            ApiError::Store(error) => {
                (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
            }
            ApiError::Internal(error) => {
                (StatusCode::INTERNAL_SERVER_ERROR, error).into_response()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{wait_for_change, AppState, WaitQuery};
    use crate::{
        config::{RuntimeConfig, ServerConfig},
        protocol::{PushRequest, SyncChange},
        store::SyncStore,
    };
    use axum::{extract::{Query, State}, http::{header, HeaderMap, HeaderValue}, Json};
    use chrono::{DateTime, Utc};
    use std::{path::PathBuf, time::Duration};

    #[tokio::test]
    async fn wait_for_change_returns_immediately_when_revision_is_newer() {
        let state = test_state("wait-immediate");
        state
            .store
            .push(PushRequest {
                device_id: "device-a".into(),
                changes: vec![sample_change("note-1", "Title", "2026-05-18T08:00:00Z")],
            })
            .expect("push note");

        let Json(response) = wait_for_change(
            State(state),
            auth_headers(),
            Query(WaitQuery {
                since: Some(0),
                timeout_seconds: Some(1),
            }),
        )
        .await
        .expect("wait response");

        assert!(response.changed);
        assert_eq!(response.revision, 1);
    }

    #[tokio::test]
    async fn wait_for_change_unblocks_after_push() {
        let state = test_state("wait-unblock");
        let wait_state = state.clone();

        let waiter = tokio::spawn(async move {
            wait_for_change(
                State(wait_state),
                auth_headers(),
                Query(WaitQuery {
                    since: Some(0),
                    timeout_seconds: Some(1),
                }),
            )
            .await
            .expect("wait response")
            .0
        });

        tokio::time::sleep(Duration::from_millis(20)).await;
        state
            .store
            .push(PushRequest {
                device_id: "device-a".into(),
                changes: vec![sample_change("note-1", "Title", "2026-05-18T08:00:00Z")],
            })
            .expect("push note");

        let response = waiter.await.expect("wait task");
        assert!(response.changed);
        assert_eq!(response.revision, 1);
    }

    #[tokio::test]
    async fn wait_for_change_times_out_without_updates() {
        let state = test_state("wait-timeout");

        let Json(response) = wait_for_change(
            State(state),
            auth_headers(),
            Query(WaitQuery {
                since: Some(0),
                timeout_seconds: Some(1),
            }),
        )
        .await
        .expect("wait response");

        assert!(!response.changed);
        assert_eq!(response.revision, 0);
    }

    fn test_state(name: &str) -> AppState {
        let base = std::env::var_os("FLORAL_SYNC_SERVER_TEST_TEMP_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("floral-sync-server-sync-api-tests"));
        let root = base.join(name);
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("remove stale root");
        }
        std::fs::create_dir_all(&root).expect("create root");
        let db_path = root.join("sync.sqlite3");
        let config = ServerConfig {
            config_path: root.join("sync-server.toml"),
            sync_listen: vec!["127.0.0.1:8787".into()],
            admin_listen: vec!["127.0.0.1:8788".into()],
            db_path,
            export_dir: root.join("exports"),
            log_path: root.join("logs").join("server.log"),
            log_level: "info".into(),
            sync_token: "sync-token".into(),
            admin_password_hash: None,
            admin_session_secret: "session-secret".into(),
        };
        let store = SyncStore::open(config.db_path.clone()).expect("open store");
        AppState::new(store, RuntimeConfig::new(config))
    }

    fn auth_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str("Bearer sync-token").expect("auth header"),
        );
        headers
    }

    fn sample_change(id: &str, title: &str, updated_at: &str) -> SyncChange {
        SyncChange {
            id: id.into(),
            title: title.into(),
            content: "body".into(),
            category: String::new(),
            created_at: parse_time("2026-05-18T08:00:00Z"),
            updated_at: parse_time(updated_at),
            deleted_at: None,
            content_hash: format!("{id}:{title}"),
            device_id: "device-a".into(),
        }
    }

    fn parse_time(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .expect("time")
            .with_timezone(&Utc)
    }
}
