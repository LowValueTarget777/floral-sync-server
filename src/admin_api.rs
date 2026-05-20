use std::{
    collections::{hash_map::DefaultHasher, HashSet},
    fs,
    hash::{Hash, Hasher},
    io::{Cursor, Write},
    path::Component,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use axum::{
    extract::{Path as AxumPath, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use zip::{write::FileOptions, CompressionMethod, ZipWriter};

use crate::{
    auth::{bootstrap_required, hash_admin_password, verify_admin_password},
    config::{
        generate_token, ConfigPatch, RuntimeConfig, RuntimeConfigSnapshot, ServerConfig,
    },
    session::{sign_session_cookie, verify_session_cookie, AdminSessionClaims},
    store::{
        AdminStore, MarkdownDownload, NoteListPage, NoteListQuery, NoteStateFilter, StoreError,
        SyncStore,
    },
};

pub const ADMIN_SESSION_COOKIE_NAME: &str = "floral_admin_session";

#[derive(Clone)]
pub struct AdminAppState {
    sync_store: SyncStore,
    admin_store: AdminStore,
    runtime_config: RuntimeConfig,
    restart_handle: RestartHandle,
}

#[derive(Clone, Default)]
pub struct RestartHandle {
    requested: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl RestartHandle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_restart(&self) {
        self.requested.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_requested(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }

    pub async fn wait_for_restart(&self) {
        if self.is_requested() {
            return;
        }

        self.notify.notified().await;
    }
}

impl AdminAppState {
    pub fn new(
        sync_store: SyncStore,
        admin_store: AdminStore,
        runtime_config: RuntimeConfig,
        restart_handle: RestartHandle,
    ) -> Self {
        Self {
            sync_store,
            admin_store,
            runtime_config,
            restart_handle,
        }
    }

    fn effective_config(&self) -> ServerConfig {
        self.runtime_config.effective_config()
    }

    fn snapshot(&self) -> RuntimeConfigSnapshot {
        self.runtime_config.snapshot()
    }

    fn update_config(&self, patch: &ConfigPatch) -> Result<RuntimeConfigSnapshot, ApiError> {
        self.runtime_config.apply_patch(patch).map_err(ApiError::from)
    }

    fn request_restart(&self) {
        self.restart_handle.request_restart();
    }
}

pub fn router(state: AdminAppState) -> Router {
    Router::new()
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/admin/api/bootstrap", post(bootstrap))
        .route("/admin/api/session", get(session_status))
        .route("/admin/api/overview", get(overview))
        .route("/admin/api/notes", get(notes))
        .route("/admin/api/notes/download.zip", get(notes_archive_download))
        .route("/admin/api/notes/:id/history", get(note_history))
        .route("/admin/api/notes/:id/download", get(note_download))
        .route("/admin/api/notes/:id", get(note_detail))
        .route("/admin/api/settings", get(settings).post(update_settings))
        .route("/admin/api/settings/token/reset", post(reset_sync_token))
        .route("/admin/api/settings/restart", post(restart_service))
        .route("/admin/api/settings/password", post(change_password))
        .route("/admin/api/maintenance/backup", post(create_backup))
        .route("/admin/api/maintenance/restore", post(restore_backup))
        .route("/admin/api/maintenance/backups", get(list_backups))
        .route("/admin/api/logs", get(read_logs))
        .merge(crate::admin_web::router())
        .with_state(state)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PasswordRequest {
    password: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PasswordChangeRequest {
    current_password: String,
    new_password: String,
    confirm_password: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsUpdateRequest {
    sync_listen: Option<Vec<String>>,
    admin_listen: Option<Vec<String>>,
    db_path: Option<String>,
    export_dir: Option<String>,
    log_path: Option<String>,
    log_level: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NotesQuery {
    page: Option<u64>,
    page_size: Option<u64>,
    search: Option<String>,
    category: Option<String>,
    state: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct NoteArchiveQuery {
    #[serde(default)]
    id: Vec<String>,
    ids: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LogsQuery {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RestoreBackupRequest {
    file_name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionResponse {
    authenticated: bool,
    bootstrap_required: bool,
    password_configured: bool,
    expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OverviewResponse {
    latest_revision: u64,
    note_count: u64,
    deleted_note_count: u64,
    category_count: u64,
    latest_snapshot_at: Option<DateTime<Utc>>,
    sync_listen: Vec<String>,
    admin_listen: Vec<String>,
    db_path: String,
    export_dir: String,
    log_path: String,
    log_level: String,
    recent_activity_summary: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NotesPageResponse {
    page: u64,
    page_size: u64,
    total: u64,
    notes: Vec<crate::store::NoteListItem>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsResponse {
    sync_listen: Vec<String>,
    admin_listen: Vec<String>,
    db_path: String,
    export_dir: String,
    log_path: String,
    log_level: String,
    sync_token: String,
    sync_token_configured: bool,
    admin_password_configured: bool,
    admin_session_secret_configured: bool,
    pending_restart_fields: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsUpdateResponse {
    settings: SettingsResponse,
    restart_required_fields: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenResetResponse {
    sync_token: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RestartResponse {
    restart_requested: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupResponse {
    file_name: String,
    path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupEntry {
    file_name: String,
    size_bytes: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RestoreBackupResponse {
    file_name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LogsResponse {
    path: String,
    lines: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

async fn login(
    State(state): State<AdminAppState>,
    headers: HeaderMap,
    Json(request): Json<PasswordRequest>,
) -> Result<Response, ApiError> {
    let config = state.effective_config();
    require_same_origin(&headers)?;
    if bootstrap_required(&config) {
        return Err(ApiError::Conflict(
            "admin bootstrap must be completed before login".into(),
        ));
    }

    let hash = config
        .admin_password_hash
        .as_deref()
        .ok_or_else(|| ApiError::Conflict("admin password is not configured".into()))?;
    let verified = verify_admin_password(hash, &request.password)
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    if !verified {
        return Err(ApiError::Unauthorized);
    }

    issue_session_response(&config)
}

async fn logout(
    State(state): State<AdminAppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let config = state.effective_config();
    require_same_origin(&headers)?;
    let body = SessionResponse {
        authenticated: false,
        bootstrap_required: bootstrap_required(&config),
        password_configured: config.admin_password_hash.is_some(),
        expires_at: None,
    };
    Ok(with_set_cookie(
        Json(body).into_response(),
        cleared_session_cookie(),
    ))
}

async fn bootstrap(
    State(state): State<AdminAppState>,
    headers: HeaderMap,
    Json(request): Json<PasswordRequest>,
) -> Result<Response, ApiError> {
    let config = state.effective_config();
    require_same_origin(&headers)?;
    if !bootstrap_required(&config) {
        return Err(ApiError::Conflict(
            "admin bootstrap has already been completed".into(),
        ));
    }

    let password_hash = hash_admin_password(&request.password)
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    let snapshot = state.update_config(
        &ConfigPatch {
            admin_password_hash: Some(password_hash),
            ..ConfigPatch::default()
        },
    )?;

    issue_session_response(&snapshot.effective)
}

async fn session_status(
    State(state): State<AdminAppState>,
    headers: HeaderMap,
) -> Result<Json<SessionResponse>, ApiError> {
    let config = state.effective_config();
    Ok(Json(current_session_response(&config, session_claims(&headers, &config))))
}

async fn overview(
    State(state): State<AdminAppState>,
    headers: HeaderMap,
) -> Result<Json<OverviewResponse>, ApiError> {
    let config = state.effective_config();
    require_session(&headers, &config)?;
    let summary = state.admin_store.overview()?;

    Ok(Json(OverviewResponse {
        latest_revision: summary.latest_revision,
        note_count: summary.note_count,
        deleted_note_count: summary.deleted_note_count,
        category_count: summary.category_count,
        latest_snapshot_at: summary.latest_snapshot_at,
        sync_listen: config.sync_listen.clone(),
        admin_listen: config.admin_listen.clone(),
        db_path: config.db_path.display().to_string(),
        export_dir: config.export_dir.display().to_string(),
        log_path: config.log_path.display().to_string(),
        log_level: config.log_level.clone(),
        recent_activity_summary: recent_activity_summary(&summary),
    }))
}

async fn notes(
    State(state): State<AdminAppState>,
    headers: HeaderMap,
    Query(query): Query<NotesQuery>,
) -> Result<Json<NotesPageResponse>, ApiError> {
    let config = state.effective_config();
    require_session(&headers, &config)?;
    let page = state.admin_store.note_list(&NoteListQuery {
        page: query.page.unwrap_or(1),
        page_size: query.page_size.unwrap_or(50),
        search: query.search,
        category: query.category,
        state: parse_note_state(query.state.as_deref())?,
    })?;

    Ok(Json(NotesPageResponse::from(page)))
}

async fn note_detail(
    State(state): State<AdminAppState>,
    headers: HeaderMap,
    AxumPath(note_id): AxumPath<String>,
) -> Result<Json<crate::store::NoteDetail>, ApiError> {
    let config = state.effective_config();
    require_session(&headers, &config)?;
    let note = state
        .admin_store
        .note_detail(&note_id)?
        .ok_or_else(|| ApiError::NotFound("note not found".into()))?;
    Ok(Json(note))
}

async fn note_history(
    State(state): State<AdminAppState>,
    headers: HeaderMap,
    AxumPath(note_id): AxumPath<String>,
) -> Result<Json<Vec<crate::store::NoteSnapshot>>, ApiError> {
    let config = state.effective_config();
    require_session(&headers, &config)?;
    Ok(Json(state.admin_store.note_history(&note_id)?))
}

async fn note_download(
    State(state): State<AdminAppState>,
    headers: HeaderMap,
    AxumPath(note_id): AxumPath<String>,
) -> Result<Response, ApiError> {
    let config = state.effective_config();
    require_session(&headers, &config)?;
    let download = state
        .admin_store
        .markdown_download(&note_id)?
        .ok_or_else(|| ApiError::NotFound("note not found".into()))?;

    let mut response = download.markdown.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/markdown; charset=utf-8"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", download.file_name))
            .map_err(|error| ApiError::Internal(error.to_string()))?,
    );
    Ok(response)
}

async fn notes_archive_download(
    State(state): State<AdminAppState>,
    headers: HeaderMap,
    Query(query): Query<NoteArchiveQuery>,
) -> Result<Response, ApiError> {
    let config = state.effective_config();
    require_session(&headers, &config)?;

    let note_ids = normalize_archive_note_ids(query);
    if note_ids.is_empty() {
        return Err(ApiError::BadRequest(
            "select at least one note to download".into(),
        ));
    }

    let mut downloads = Vec::with_capacity(note_ids.len());
    for note_id in note_ids {
        let download = state
            .admin_store
            .markdown_download(&note_id)?
            .ok_or_else(|| ApiError::NotFound(format!("note {note_id} not found")))?;
        downloads.push(download);
    }

    let archive = build_notes_archive(downloads)?;
    let mut response = archive.bytes.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", archive.file_name))
            .map_err(|error| ApiError::Internal(error.to_string()))?,
    );
    Ok(response)
}

async fn settings(
    State(state): State<AdminAppState>,
    headers: HeaderMap,
) -> Result<Json<SettingsResponse>, ApiError> {
    let snapshot = state.snapshot();
    require_session(&headers, &snapshot.effective)?;
    Ok(Json(settings_response(&snapshot)))
}

async fn update_settings(
    State(state): State<AdminAppState>,
    headers: HeaderMap,
    Json(request): Json<SettingsUpdateRequest>,
) -> Result<Json<SettingsUpdateResponse>, ApiError> {
    let config = state.effective_config();
    require_session(&headers, &config)?;
    require_same_origin(&headers)?;

    let snapshot = state.update_config(
        &ConfigPatch {
            listen: request.sync_listen,
            admin_listen: request.admin_listen,
            db_path: request.db_path.map(PathBuf::from),
            export_dir: request.export_dir.map(PathBuf::from),
            log_path: request.log_path.map(PathBuf::from),
            log_level: request.log_level,
            token: None,
            admin_password_hash: None,
        },
    )?;

    Ok(Json(SettingsUpdateResponse {
        settings: settings_response(&snapshot),
        restart_required_fields: snapshot.pending_restart_fields.clone(),
    }))
}

async fn reset_sync_token(
    State(state): State<AdminAppState>,
    headers: HeaderMap,
) -> Result<Json<TokenResetResponse>, ApiError> {
    let config = state.effective_config();
    require_session(&headers, &config)?;
    require_same_origin(&headers)?;

    let sync_token = generate_token();
    state.update_config(
        &ConfigPatch {
            token: Some(sync_token.clone()),
            ..ConfigPatch::default()
        },
    )?;

    Ok(Json(TokenResetResponse { sync_token }))
}

async fn restart_service(
    State(state): State<AdminAppState>,
    headers: HeaderMap,
) -> Result<Json<RestartResponse>, ApiError> {
    let config = state.effective_config();
    require_session(&headers, &config)?;
    require_same_origin(&headers)?;

    state.request_restart();

    Ok(Json(RestartResponse {
        restart_requested: true,
    }))
}

async fn change_password(
    State(state): State<AdminAppState>,
    headers: HeaderMap,
    Json(request): Json<PasswordChangeRequest>,
) -> Result<Response, ApiError> {
    let config = state.effective_config();
    require_session(&headers, &config)?;
    require_same_origin(&headers)?;
    if bootstrap_required(&config) {
        return Err(ApiError::Conflict(
            "admin bootstrap must be completed before changing the password".into(),
        ));
    }
    if request.new_password != request.confirm_password {
        return Err(ApiError::BadRequest("new password confirmation does not match".into()));
    }

    let current_hash = config
        .admin_password_hash
        .as_deref()
        .ok_or_else(|| ApiError::Conflict("admin password is not configured".into()))?;
    let verified = verify_admin_password(current_hash, &request.current_password)
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    if !verified {
        return Err(ApiError::Unauthorized);
    }

    let password_hash = hash_admin_password(&request.new_password)
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    let snapshot = state.update_config(
        &ConfigPatch {
            admin_password_hash: Some(password_hash),
            ..ConfigPatch::default()
        },
    )?;

    issue_session_response(&snapshot.effective)
}

async fn create_backup(
    State(state): State<AdminAppState>,
    headers: HeaderMap,
) -> Result<Json<BackupResponse>, ApiError> {
    let config = state.effective_config();
    require_session(&headers, &config)?;
    require_same_origin(&headers)?;
    fs::create_dir_all(&config.export_dir)?;

    let file_name = format!(
        "floral-sync-{}.sqlite3",
        Utc::now().format("%Y-%m-%dT%H-%M-%SZ")
    );
    let destination = config.export_dir.join(&file_name);
    state.sync_store.backup_to(&destination)?;

    Ok(Json(BackupResponse {
        file_name,
        path: destination.display().to_string(),
    }))
}

async fn list_backups(
    State(state): State<AdminAppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<BackupEntry>>, ApiError> {
    let config = state.effective_config();
    require_session(&headers, &config)?;
    if !config.export_dir.exists() {
        return Ok(Json(Vec::new()));
    }

    let mut backups = fs::read_dir(&config.export_dir)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() {
                return None;
            }
            Some(BackupEntry {
                file_name: entry.file_name().to_string_lossy().to_string(),
                size_bytes: metadata.len(),
            })
        })
        .collect::<Vec<_>>();
    backups.sort_by(|left, right| right.file_name.cmp(&left.file_name));
    Ok(Json(backups))
}

async fn restore_backup(
    State(state): State<AdminAppState>,
    headers: HeaderMap,
    Json(request): Json<RestoreBackupRequest>,
) -> Result<Json<RestoreBackupResponse>, ApiError> {
    let config = state.effective_config();
    require_session(&headers, &config)?;
    require_same_origin(&headers)?;

    let backup_path = resolve_backup_path(&config.export_dir, &request.file_name)?;
    state.sync_store.restore_from(&backup_path)?;

    Ok(Json(RestoreBackupResponse {
        file_name: request.file_name.trim().to_string(),
    }))
}

async fn read_logs(
    State(state): State<AdminAppState>,
    headers: HeaderMap,
    Query(query): Query<LogsQuery>,
) -> Result<Json<LogsResponse>, ApiError> {
    let config = state.effective_config();
    require_session(&headers, &config)?;
    let limit = query.limit.unwrap_or(200).clamp(1, 1000);
    let contents = match fs::read_to_string(&config.log_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(ApiError::Io(error)),
    };
    let mut lines = contents
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if lines.len() > limit {
        lines = lines.split_off(lines.len() - limit);
    }

    Ok(Json(LogsResponse {
        path: config.log_path.display().to_string(),
        lines,
    }))
}

fn settings_response(snapshot: &RuntimeConfigSnapshot) -> SettingsResponse {
    let config = &snapshot.effective;
    SettingsResponse {
        sync_listen: config.sync_listen.clone(),
        admin_listen: config.admin_listen.clone(),
        db_path: config.db_path.display().to_string(),
        export_dir: config.export_dir.display().to_string(),
        log_path: config.log_path.display().to_string(),
        log_level: config.log_level.clone(),
        sync_token: config.sync_token.clone(),
        sync_token_configured: !config.sync_token.trim().is_empty(),
        admin_password_configured: config.admin_password_hash.is_some(),
        admin_session_secret_configured: !config.admin_session_secret.trim().is_empty(),
        pending_restart_fields: snapshot.pending_restart_fields.clone(),
    }
}

fn resolve_backup_path(export_dir: &PathBuf, file_name: &str) -> Result<PathBuf, ApiError> {
    let trimmed = file_name.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("backup file name is required".into()));
    }

    let candidate = PathBuf::from(trimmed);
    let mut components = candidate.components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(ApiError::BadRequest(
            "backup file name must not include path segments".into(),
        ));
    }

    let resolved = export_dir.join(trimmed);
    if !resolved.is_file() {
        return Err(ApiError::NotFound("backup file was not found".into()));
    }

    Ok(resolved)
}

fn normalize_archive_note_ids(query: NoteArchiveQuery) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();

    let mut raw_ids = query.id;
    if let Some(ids) = query.ids {
        raw_ids.extend(ids.split(',').map(str::to_string));
    }

    for note_id in raw_ids {
        let trimmed = note_id.trim();
        if trimmed.is_empty() {
            continue;
        }

        let normalized_id = trimmed.to_string();
        if seen.insert(normalized_id.clone()) {
            normalized.push(normalized_id);
        }
    }

    normalized
}

struct NotesArchive {
    file_name: String,
    bytes: Vec<u8>,
}

fn build_notes_archive(downloads: Vec<MarkdownDownload>) -> Result<NotesArchive, ApiError> {
    let mut used_file_names = HashSet::new();
    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = FileOptions::default().compression_method(CompressionMethod::Deflated);

    for download in downloads {
        let file_name = unique_archive_file_name(&download.file_name, &mut used_file_names);
        zip.start_file(file_name, options)
            .map_err(|error| ApiError::Internal(error.to_string()))?;
        zip.write_all(download.markdown.as_bytes())?;
    }

    let bytes = zip
        .finish()
        .map_err(|error| ApiError::Internal(error.to_string()))?
        .into_inner();
    Ok(NotesArchive {
        file_name: format!(
            "floral-sync-notes-{}.zip",
            Utc::now().format("%Y-%m-%dT%H-%M-%SZ")
        ),
        bytes,
    })
}

fn unique_archive_file_name(file_name: &str, used_file_names: &mut HashSet<String>) -> String {
    if used_file_names.insert(file_name.to_string()) {
        return file_name.to_string();
    }

    let (stem, extension) = match file_name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem, Some(extension)),
        _ => (file_name, None),
    };

    let mut suffix = 2;
    loop {
        let candidate = match extension {
            Some(extension) => format!("{stem}-{suffix}.{extension}"),
            None => format!("{stem}-{suffix}"),
        };

        if used_file_names.insert(candidate.clone()) {
            return candidate;
        }

        suffix += 1;
    }
}

fn recent_activity_summary(summary: &crate::store::AdminOverview) -> String {
    match summary.latest_snapshot_at {
        Some(timestamp) => format!("Latest snapshot captured at {}", timestamp.to_rfc3339()),
        None => "No note snapshots have been captured yet".into(),
    }
}

fn issue_session_response(config: &ServerConfig) -> Result<Response, ApiError> {
    let claims = AdminSessionClaims::new(generate_token(), Utc::now(), password_version(config));
    let cookie_value = sign_session_cookie(&config.admin_session_secret, &claims)
        .map_err(|error| ApiError::Internal(error.to_string()))?;
    let body = SessionResponse {
        authenticated: true,
        bootstrap_required: false,
        password_configured: true,
        expires_at: Some(claims.expires_at()),
    };
    Ok(with_set_cookie(
        Json(body).into_response(),
        session_cookie(&cookie_value),
    ))
}

fn current_session_response(
    config: &ServerConfig,
    claims: Option<AdminSessionClaims>,
) -> SessionResponse {
    SessionResponse {
        authenticated: claims.is_some(),
        bootstrap_required: bootstrap_required(config),
        password_configured: config.admin_password_hash.is_some(),
        expires_at: claims.map(|claims| claims.expires_at()),
    }
}

fn require_session(headers: &HeaderMap, config: &ServerConfig) -> Result<AdminSessionClaims, ApiError> {
    session_claims(headers, config).ok_or(ApiError::Unauthorized)
}

fn require_same_origin(headers: &HeaderMap) -> Result<(), ApiError> {
    let origin = headers
        .get(header::ORIGIN)
        .ok_or_else(|| ApiError::Forbidden("origin header is required for POST requests".into()))?
        .to_str()
        .map_err(|_| ApiError::Forbidden("origin header is invalid".into()))?;
    let parsed = ParsedOrigin::parse(origin)
        .ok_or_else(|| ApiError::Forbidden("origin header is invalid".into()))?;

    let expected = request_origin_from_headers(headers)
        .ok_or_else(|| ApiError::Forbidden("host header is required for POST requests".into()))?;

    if parsed.matches(&expected) {
        Ok(())
    } else {
        Err(ApiError::Forbidden(
            "origin header does not match an allowed admin host".into(),
        ))
    }
}

fn session_claims(headers: &HeaderMap, config: &ServerConfig) -> Option<AdminSessionClaims> {
    let cookie_value = read_cookie(headers, ADMIN_SESSION_COOKIE_NAME)?;
    verify_session_cookie(
        &config.admin_session_secret,
        cookie_value,
        Utc::now(),
        password_version(config),
    )
    .ok()
}

fn password_version(config: &ServerConfig) -> u64 {
    let Some(hash) = config.admin_password_hash.as_ref() else {
        return 0;
    };

    let mut hasher = DefaultHasher::new();
    hash.hash(&mut hasher);
    hasher.finish()
}

fn read_cookie<'a>(headers: &'a HeaderMap, cookie_name: &str) -> Option<&'a str> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    raw.split(';').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        if name == cookie_name {
            Some(value)
        } else {
            None
        }
    })
}

fn session_cookie(cookie_value: &str) -> String {
    format!(
        "{ADMIN_SESSION_COOKIE_NAME}={cookie_value}; Path=/; HttpOnly; SameSite=Strict"
    )
}

fn cleared_session_cookie() -> String {
    format!(
        "{ADMIN_SESSION_COOKIE_NAME}=; Path=/; Max-Age=0; HttpOnly; SameSite=Strict"
    )
}

fn with_set_cookie(mut response: Response, set_cookie: String) -> Response {
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&set_cookie).expect("valid session cookie"),
    );
    response
}

fn parse_note_state(value: Option<&str>) -> Result<NoteStateFilter, ApiError> {
    match value.unwrap_or("all") {
        "all" => Ok(NoteStateFilter::All),
        "active" => Ok(NoteStateFilter::Active),
        "deleted" => Ok(NoteStateFilter::Deleted),
        other => Err(ApiError::BadRequest(format!(
            "invalid note state filter: {other}"
        ))),
    }
}

struct ParsedOrigin {
    scheme: String,
    host: String,
    port: u16,
}

impl ParsedOrigin {
    fn parse(value: &str) -> Option<Self> {
        let (scheme, rest) = value.split_once("://")?;
        let authority = rest.split(['/', '?', '#']).next()?;
        Self::from_authority(scheme, authority)
    }

    fn from_authority(scheme: &str, authority: &str) -> Option<Self> {
        if authority.is_empty() || authority == "null" {
            return None;
        }

        if let Some(remainder) = authority.strip_prefix('[') {
            let end = remainder.find(']')?;
            let host = &remainder[..end];
            let after_host = &remainder[end + 1..];
            let port = after_host
                .strip_prefix(':')
                .and_then(|port| port.parse().ok())
                .or_else(|| default_port_for_scheme(scheme))?;
            return Some(Self {
                scheme: scheme.to_string(),
                host: host.to_string(),
                port,
            });
        }

        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) if !host.is_empty() => (host, port.parse().ok()?),
            _ => (authority, default_port_for_scheme(scheme)?),
        };

        Some(Self {
            scheme: scheme.to_string(),
            host: host.to_string(),
            port,
        })
    }

    fn matches(&self, other: &Self) -> bool {
        self.port == other.port
            && self.scheme.eq_ignore_ascii_case(&other.scheme)
            && self.host.eq_ignore_ascii_case(&other.host)
    }
}

fn request_origin_from_headers(headers: &HeaderMap) -> Option<ParsedOrigin> {
    let scheme = forwarded_header_value(headers, "x-forwarded-proto").unwrap_or("http");
    let authority = forwarded_header_value(headers, "x-forwarded-host").or_else(|| {
        headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
    })?;
    ParsedOrigin::from_authority(scheme, authority)
}

fn forwarded_header_value<'a>(headers: &'a HeaderMap, name: &'static str) -> Option<&'a str> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn default_port_for_scheme(scheme: &str) -> Option<u16> {
    match scheme {
        "http" => Some(80),
        "https" => Some(443),
        _ => None,
    }
}

impl From<NoteListPage> for NotesPageResponse {
    fn from(value: NoteListPage) -> Self {
        Self {
            page: value.page,
            page_size: value.page_size,
            total: value.total,
            notes: value.notes,
        }
    }
}

enum ApiError {
    BadRequest(String),
    Unauthorized,
    Forbidden(String),
    NotFound(String),
    Conflict(String),
    Config(crate::config::ConfigError),
    Store(StoreError),
    Io(std::io::Error),
    Internal(String),
}

impl From<crate::config::ConfigError> for ApiError {
    fn from(error: crate::config::ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<std::io::Error> for ApiError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized".into()),
            Self::Forbidden(message) => (StatusCode::FORBIDDEN, message),
            Self::NotFound(message) => (StatusCode::NOT_FOUND, message),
            Self::Conflict(message) => (StatusCode::CONFLICT, message),
            Self::Config(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
            Self::Store(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
            Self::Io(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
            Self::Internal(message) => (StatusCode::INTERNAL_SERVER_ERROR, message),
        };
        (status, Json(ErrorResponse { error: message })).into_response()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{Cursor, Read},
        path::PathBuf,
    };

    use axum::{
        body::{to_bytes, Body},
        http::{header, Request, StatusCode},
        routing::get,
        Router,
    };
    use chrono::{DateTime, Utc};
    use serde_json::Value;
    use tower::ServiceExt;
    use zip::ZipArchive;

    use super::{router, AdminAppState, RestartHandle, ADMIN_SESSION_COOKIE_NAME};
    use crate::{
        auth::hash_admin_password,
        config::{load_or_create_config, ConfigOverrides, RuntimeConfig, ServerConfig},
        protocol::{PushRequest, SyncChange},
        sync_api::{health, AppState as SyncAppState},
        store::{AdminStore, SyncStore},
    };

    #[tokio::test]
    async fn login_returns_session_cookie_for_valid_password() {
        let fixture = AdminApiFixture::bootstrapped().await;

        let response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/login")
                    .header(header::HOST, fixture.admin_host())
                    .header(header::ORIGIN, fixture.admin_origin())
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"password":"hunter2"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response
                .headers()
                .get(header::SET_COOKIE)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.contains(ADMIN_SESSION_COOKIE_NAME))
        );
        let body = json_body(response).await;
        assert_eq!(body["authenticated"], Value::Bool(true));
        assert_eq!(body["bootstrapRequired"], Value::Bool(false));
    }

    #[tokio::test]
    async fn bootstrap_sets_admin_password_and_session_cookie() {
        let fixture = AdminApiFixture::bootstrap_required().await;

        let response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/api/bootstrap")
                    .header(header::HOST, fixture.admin_host())
                    .header(header::ORIGIN, fixture.admin_origin())
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"password":"new-password"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response
                .headers()
                .get(header::SET_COOKIE)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.contains(ADMIN_SESSION_COOKIE_NAME))
        );

        let config = load_or_create_config(&fixture.config_path, &ConfigOverrides::default())
            .expect("reload config");
        assert!(config.admin_password_hash.is_some());
    }

    #[tokio::test]
    async fn session_route_returns_authenticated_session_state() {
        let fixture = AdminApiFixture::bootstrapped().await;
        let cookie = fixture.login_cookie("hunter2").await;

        let response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/api/session")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["authenticated"], Value::Bool(true));
        assert_eq!(body["bootstrapRequired"], Value::Bool(false));
        assert_eq!(body["passwordConfigured"], Value::Bool(true));
    }

    #[tokio::test]
    async fn protected_admin_route_requires_session() {
        let fixture = AdminApiFixture::bootstrapped().await;

        let response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/api/overview")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn overview_route_returns_counts_and_listener_info() {
        let fixture = AdminApiFixture::bootstrapped().await;
        let cookie = fixture.login_cookie("hunter2").await;

        let response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/api/overview")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["noteCount"], Value::from(1));
        assert_eq!(body["latestRevision"], Value::from(1));
        assert_eq!(body["syncListen"][0], Value::from("127.0.0.1:8787"));
        assert_eq!(body["adminListen"][0], Value::from("127.0.0.1:8788"));
    }

    #[tokio::test]
    async fn notes_route_returns_paginated_note_rows() {
        let fixture = AdminApiFixture::bootstrapped().await;
        let cookie = fixture.login_cookie("hunter2").await;

        let response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/api/notes?page=1&pageSize=10")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["total"], Value::from(1));
        assert_eq!(body["notes"][0]["id"], Value::from("note-1"));
    }

    #[tokio::test]
    async fn note_detail_route_returns_current_note_state() {
        let fixture = AdminApiFixture::bootstrapped().await;
        let cookie = fixture.login_cookie("hunter2").await;

        let response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/api/notes/note-1")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["id"], Value::from("note-1"));
        assert_eq!(body["title"], Value::from("First note"));
    }

    #[tokio::test]
    async fn note_history_route_returns_snapshot_timeline() {
        let fixture = AdminApiFixture::bootstrapped().await;
        let cookie = fixture.login_cookie("hunter2").await;

        let response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/api/notes/note-1/history")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body[0]["noteId"], Value::from("note-1"));
        assert_eq!(body[0]["revision"], Value::from(1));
    }

    #[tokio::test]
    async fn notes_archive_download_returns_zip_for_selected_notes() {
        let fixture = AdminApiFixture::bootstrapped().await;
        let cookie = fixture.login_cookie("hunter2").await;

        let store = SyncStore::open(fixture.config.db_path.clone()).expect("open live store");
        store
            .push(PushRequest {
                device_id: "device-b".into(),
                changes: vec![SyncChange {
                    id: "note-2".into(),
                    title: "Second note".into(),
                    content: "second body".into(),
                    category: "Archive".into(),
                    created_at: parse_time("2026-05-19T11:00:00Z"),
                    updated_at: parse_time("2026-05-19T11:05:00Z"),
                    deleted_at: None,
                    content_hash: "note-2:1".into(),
                    device_id: "device-b".into(),
                }],
            })
            .expect("seed second note");

        let response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/api/notes/download.zip?ids=note-1,note-2")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("application/zip")));
        assert!(response
            .headers()
            .get(header::CONTENT_DISPOSITION)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("floral-sync-notes-")));

        let entries = zip_entries(response).await;
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, "first-note.md");
        assert!(entries[0].1.contains("# First note"));
        assert!(entries[0].1.contains("body"));
        assert_eq!(entries[1].0, "second-note.md");
        assert!(entries[1].1.contains("# Second note"));
        assert!(entries[1].1.contains("second body"));
    }

    #[tokio::test]
    async fn notes_archive_download_rejects_empty_selection() {
        let fixture = AdminApiFixture::bootstrapped().await;
        let cookie = fixture.login_cookie("hunter2").await;

        let response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/api/notes/download.zip")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = json_body(response).await;
        assert_eq!(
            body["error"],
            Value::from("select at least one note to download")
        );
    }

    #[tokio::test]
    async fn restore_backup_replaces_current_note_state() {
        let fixture = AdminApiFixture::bootstrapped().await;
        let cookie = fixture.login_cookie("hunter2").await;

        let backup_response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/api/maintenance/backup")
                    .header(header::COOKIE, cookie.clone())
                    .header(header::HOST, fixture.admin_host())
                    .header(header::ORIGIN, fixture.admin_origin())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(backup_response.status(), StatusCode::OK);
        let backup_body = json_body(backup_response).await;
        let backup_file = backup_body["fileName"].as_str().expect("backup file name");

        let store = SyncStore::open(fixture.config.db_path.clone()).expect("open live store");
        store
            .push(PushRequest {
                device_id: "device-b".into(),
                changes: vec![SyncChange {
                    id: "note-1".into(),
                    title: "Changed after backup".into(),
                    content: "updated body".into(),
                    category: String::new(),
                    created_at: parse_time("2026-05-19T07:00:00Z"),
                    updated_at: parse_time("2026-05-19T09:00:00Z"),
                    deleted_at: None,
                    content_hash: "note-1:changed-after-backup".into(),
                    device_id: "device-b".into(),
                }],
            })
            .expect("push changed state");

        let restore_response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/api/maintenance/restore")
                    .header(header::COOKIE, cookie.clone())
                    .header(header::HOST, fixture.admin_host())
                    .header(header::ORIGIN, fixture.admin_origin())
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(r#"{{"fileName":"{backup_file}"}}"#)))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(restore_response.status(), StatusCode::OK);

        let note_response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/api/notes/note-1")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(note_response.status(), StatusCode::OK);
        let note_body = json_body(note_response).await;
        assert_eq!(note_body["title"], Value::from("First note"));
        assert_eq!(note_body["content"], Value::from("body"));
        assert!(
            note_body["revision"].as_u64().expect("note revision") > 1,
            "restore should replay the restored state at a newer revision"
        );
    }

    #[tokio::test]
    async fn restore_backup_rejects_path_segments() {
        let fixture = AdminApiFixture::bootstrapped().await;
        let cookie = fixture.login_cookie("hunter2").await;

        let response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/api/maintenance/restore")
                    .header(header::COOKIE, cookie)
                    .header(header::HOST, fixture.admin_host())
                    .header(header::ORIGIN, fixture.admin_origin())
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"fileName":"../escape.sqlite3"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = json_body(response).await;
        assert_eq!(
            body["error"],
            Value::from("backup file name must not include path segments")
        );
    }

    #[tokio::test]
    async fn admin_entry_route_serves_embedded_html_shell() {
        let fixture = AdminApiFixture::bootstrapped().await;

        let response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("text/html")));

        let body = text_body(response).await;
        assert!(body.contains("<div id=\"root\"></div>"));
        assert!(body.contains("/assets/"));
    }

    #[tokio::test]
    async fn admin_entry_html_references_embedded_assets_that_are_served() {
        let fixture = AdminApiFixture::bootstrapped().await;

        let index_response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let html = text_body(index_response).await;
        let asset_path = extract_asset_path(&html).expect("asset path");

        let asset_response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(asset_path.as_str())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(asset_response.status(), StatusCode::OK);
        assert!(asset_response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("javascript") || value.contains("css")));
    }

    #[tokio::test]
    async fn login_rejects_missing_origin_header() {
        let fixture = AdminApiFixture::bootstrapped().await;

        let response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/login")
                    .header(header::HOST, fixture.admin_host())
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"password":"hunter2"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = json_body(response).await;
        assert_eq!(
            body["error"],
            Value::from("origin header is required for POST requests")
        );
    }

    #[tokio::test]
    async fn token_reset_rejects_mismatched_origin_header() {
        let fixture = AdminApiFixture::bootstrapped().await;
        let cookie = fixture.login_cookie("hunter2").await;

        let response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/api/settings/token/reset")
                    .header(header::COOKIE, cookie)
                    .header(header::HOST, fixture.admin_host())
                    .header(header::ORIGIN, "http://evil.example:8788")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = json_body(response).await;
        assert_eq!(
            body["error"],
            Value::from("origin header does not match an allowed admin host")
        );
    }

    #[tokio::test]
    async fn login_accepts_localhost_origin_for_loopback_listener() {
        let fixture = AdminApiFixture::bootstrapped().await;

        let response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/login")
                    .header(header::HOST, "localhost:8788")
                    .header(header::ORIGIN, "http://localhost:8788")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"password":"hunter2"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn login_accepts_reverse_proxy_origin_with_forwarded_headers() {
        let fixture = AdminApiFixture::bootstrapped().await;

        let response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/login")
                    .header(header::HOST, fixture.admin_host())
                    .header("x-forwarded-host", "admin.notes.example.com")
                    .header("x-forwarded-proto", "https")
                    .header(header::ORIGIN, "https://admin.notes.example.com")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"password":"hunter2"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn token_reset_persists_new_sync_token() {
        let fixture = AdminApiFixture::bootstrapped().await;
        let cookie = fixture.login_cookie("hunter2").await;
        let previous_token = fixture.config.sync_token.clone();

        let response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/api/settings/token/reset")
                    .header(header::COOKIE, cookie)
                    .header(header::HOST, fixture.admin_host())
                    .header(header::ORIGIN, fixture.admin_origin())
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        let new_token = body["syncToken"].as_str().expect("sync token");
        assert_ne!(new_token, previous_token);

        let config = load_or_create_config(&fixture.config_path, &ConfigOverrides::default())
            .expect("reload config");
        assert_eq!(config.sync_token, new_token);
    }

    #[tokio::test]
    async fn token_reset_updates_sync_auth_immediately() {
        let fixture = AdminApiFixture::bootstrapped().await;
        let cookie = fixture.login_cookie("hunter2").await;
        let previous_token = fixture.config.sync_token.clone();

        let response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/api/settings/token/reset")
                    .header(header::COOKIE, cookie)
                    .header(header::HOST, fixture.admin_host())
                    .header(header::ORIGIN, fixture.admin_origin())
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = json_body(response).await;
        let new_token = body["syncToken"].as_str().expect("sync token").to_string();

        let old_response = fixture.sync_health(&previous_token).await;
        let new_response = fixture.sync_health(&new_token).await;

        assert_eq!(old_response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(new_response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn restart_required_settings_stay_pending_while_runtime_paths_remain_effective() {
        let fixture = AdminApiFixture::bootstrapped().await;
        let cookie = fixture.login_cookie("hunter2").await;
        let old_db_path = fixture.config.db_path.display().to_string();
        let old_export_dir = fixture.config.export_dir.display().to_string();
        let old_log_path = fixture.config.log_path.display().to_string();
        let new_db_path = fixture.root.join("pending").join("sync.sqlite3");
        let new_export_dir = fixture.root.join("pending-exports");
        let new_log_path = fixture.root.join("pending-logs").join("server.log");

        let update_response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/api/settings")
                    .header(header::COOKIE, cookie.clone())
                    .header(header::HOST, fixture.admin_host())
                    .header(header::ORIGIN, fixture.admin_origin())
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(
                        r#"{{"dbPath":"{}","exportDir":"{}","logPath":"{}","syncListen":["127.0.0.1:9999"]}}"#,
                        toml_string(&new_db_path),
                        toml_string(&new_export_dir),
                        toml_string(&new_log_path)
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(update_response.status(), StatusCode::OK);
        let update_body = json_body(update_response).await;
        assert_eq!(update_body["restartRequiredFields"][0], Value::from("syncListen"));

        let settings_response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/api/settings")
                    .header(header::COOKIE, cookie.clone())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let settings_body = json_body(settings_response).await;
        assert_eq!(settings_body["dbPath"], Value::from(old_db_path.clone()));
        assert_eq!(settings_body["exportDir"], Value::from(old_export_dir.clone()));
        assert_eq!(settings_body["logPath"], Value::from(old_log_path.clone()));
        assert_eq!(settings_body["syncToken"], Value::from(fixture.config.sync_token.clone()));
        assert_eq!(settings_body["pendingRestartFields"][0], Value::from("syncListen"));
        assert_eq!(settings_body["pendingRestartFields"][1], Value::from("dbPath"));

        let backup_response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/api/maintenance/backup")
                    .header(header::COOKIE, cookie)
                    .header(header::HOST, fixture.admin_host())
                    .header(header::ORIGIN, fixture.admin_origin())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(backup_response.status(), StatusCode::OK);
        let backup_body = json_body(backup_response).await;
        assert!(
            backup_body["path"]
                .as_str()
                .is_some_and(|path| path.starts_with(&old_export_dir))
        );

        let persisted = load_or_create_config(&fixture.config_path, &ConfigOverrides::default())
            .expect("reload config");
        assert_eq!(persisted.db_path, new_db_path);
        assert_eq!(persisted.export_dir, new_export_dir);
        assert_eq!(persisted.log_path, new_log_path);
    }

    #[tokio::test]
    async fn restart_endpoint_requests_restart() {
        let fixture = AdminApiFixture::bootstrapped().await;
        let cookie = fixture.login_cookie("hunter2").await;

        let response = fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/api/settings/restart")
                    .header(header::COOKIE, cookie)
                    .header(header::HOST, fixture.admin_host())
                    .header(header::ORIGIN, fixture.admin_origin())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["restartRequested"], Value::from(true));
        assert!(fixture.restart_requested());
    }

    async fn json_body(response: axum::response::Response) -> Value {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        serde_json::from_slice(&bytes).expect("json body")
    }

    async fn text_body(response: axum::response::Response) -> String {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        String::from_utf8(bytes.to_vec()).expect("utf8 body")
    }

    async fn zip_entries(response: axum::response::Response) -> Vec<(String, String)> {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read zip body");
        let cursor = Cursor::new(bytes.to_vec());
        let mut archive = ZipArchive::new(cursor).expect("open zip archive");
        let mut entries = Vec::new();

        for index in 0..archive.len() {
            let mut file = archive.by_index(index).expect("zip entry");
            let mut contents = String::new();
            file.read_to_string(&mut contents).expect("read zip entry");
            entries.push((file.name().to_string(), contents));
        }

        entries
    }

    fn extract_asset_path(html: &str) -> Option<String> {
        for marker in ["src=\"/assets/", "href=\"/assets/"] {
            let start = html.find(marker)? + marker.len() - "/assets/".len();
            let tail = &html[start..];
            let end = tail.find('"')?;
            return Some(tail[..end].to_string());
        }
        None
    }

    struct AdminApiFixture {
        app: Router,
        sync_app: Router,
        config: ServerConfig,
        config_path: PathBuf,
        root: PathBuf,
        restart_handle: RestartHandle,
    }

    impl AdminApiFixture {
        async fn bootstrapped() -> Self {
            Self::new(true).await
        }

        async fn bootstrap_required() -> Self {
            Self::new(false).await
        }

        async fn new(with_password: bool) -> Self {
            let temp = tempfile::tempdir().expect("temp dir");
            let root = temp.path().to_path_buf();
            let config_path = root.join("sync-server.toml");
            let db_path = root.join("data").join("sync.sqlite3");
            let export_dir = root.join("exports");
            let log_path = root.join("logs").join("floral-sync-server.log");

            fs::create_dir_all(log_path.parent().expect("log dir")).expect("create log dir");
            fs::write(&log_path, "2026-05-19T10:00:00Z INFO test log line\n").expect("write log");
            write_config(
                &config_path,
                &db_path,
                &export_dir,
                &log_path,
                with_password.then(|| hash_admin_password("hunter2").expect("password hash")),
            );
            let config = load_or_create_config(&config_path, &ConfigOverrides::default())
                .expect("load config");
            let sync_store = SyncStore::open(config.db_path.clone()).expect("open store");
            seed_store(&sync_store);
            let admin_store =
                AdminStore::open_shared(sync_store.connection()).expect("open admin store");
            let runtime_config = RuntimeConfig::new(config.clone());
            let restart_handle = RestartHandle::new();
            let app = router(AdminAppState::new(
                sync_store.clone(),
                admin_store,
                runtime_config.clone(),
                restart_handle.clone(),
            ));
            let sync_app = Router::new()
                .route("/health", get(health))
                .with_state(SyncAppState::new(
                    sync_store,
                    runtime_config,
                ));

            let _keep_tempdir = Box::leak(Box::new(temp));
            Self {
                app,
                sync_app,
                config,
                config_path,
                root,
                restart_handle,
            }
        }

        async fn login_cookie(&self, password: &str) -> String {
            let response = self
                .app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/login")
                        .header(header::HOST, self.admin_host())
                        .header(header::ORIGIN, self.admin_origin())
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(format!(r#"{{"password":"{password}"}}"#)))
                        .unwrap(),
                )
                .await
                .unwrap();

            response
                .headers()
                .get(header::SET_COOKIE)
                .and_then(|value| value.to_str().ok())
                .and_then(cookie_header_value)
                .expect("session cookie")
        }

        fn admin_origin(&self) -> String {
            format!("http://{}", self.config.admin_listen[0])
        }

        fn admin_host(&self) -> &str {
            &self.config.admin_listen[0]
        }

        async fn sync_health(&self, token: &str) -> axum::response::Response {
            self.sync_app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/health")
                        .header(header::AUTHORIZATION, format!("Bearer {token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
        }

        fn restart_requested(&self) -> bool {
            self.restart_handle.is_requested()
        }
    }

    fn cookie_header_value(set_cookie: &str) -> Option<String> {
        let (name_value, _) = set_cookie.split_once(';')?;
        Some(name_value.to_string())
    }

    fn seed_store(store: &SyncStore) {
        store
            .push(PushRequest {
                device_id: "device-a".into(),
                changes: vec![SyncChange {
                    id: "note-1".into(),
                    title: "First note".into(),
                    content: "body".into(),
                    category: "Work".into(),
                    created_at: parse_time("2026-05-19T10:00:00Z"),
                    updated_at: parse_time("2026-05-19T10:00:00Z"),
                    deleted_at: None,
                    content_hash: "note-1:1".into(),
                    device_id: "device-a".into(),
                }],
            })
            .expect("seed note");
    }

    fn parse_time(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .expect("time")
            .with_timezone(&Utc)
    }

    fn write_config(
        config_path: &PathBuf,
        db_path: &PathBuf,
        export_dir: &PathBuf,
        log_path: &PathBuf,
        admin_password_hash: Option<String>,
    ) {
        let mut contents = format!(
            "sync_listen = [\"127.0.0.1:8787\"]\nadmin_listen = [\"127.0.0.1:8788\"]\ndb_path = \"{}\"\nexport_dir = \"{}\"\nlog_path = \"{}\"\nlog_level = \"info\"\nsync_token = \"sync-token\"\nadmin_session_secret = \"session-secret\"\n",
            toml_string(db_path),
            toml_string(export_dir),
            toml_string(log_path)
        );
        if let Some(hash) = admin_password_hash {
            contents.push_str(&format!("admin_password_hash = \"{hash}\"\n"));
        }
        fs::write(config_path, contents).expect("write config");
    }

    fn toml_string(path: &PathBuf) -> String {
        path.to_string_lossy().replace('\\', "\\\\")
    }
}
