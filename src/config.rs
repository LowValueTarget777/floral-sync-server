use serde::{Deserialize, Serialize};
use std::{
    env, fs, io,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigOverrides {
    pub listen: Option<Vec<String>>,
    pub db_path: Option<PathBuf>,
    pub token: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigPatch {
    pub listen: Option<Vec<String>>,
    pub admin_listen: Option<Vec<String>>,
    pub db_path: Option<PathBuf>,
    pub export_dir: Option<PathBuf>,
    pub log_path: Option<PathBuf>,
    pub log_level: Option<String>,
    pub token: Option<String>,
    pub admin_password_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub config_path: PathBuf,
    pub sync_listen: Vec<String>,
    pub admin_listen: Vec<String>,
    pub db_path: PathBuf,
    pub export_dir: PathBuf,
    pub log_path: PathBuf,
    pub log_level: String,
    pub sync_token: String,
    pub admin_password_hash: Option<String>,
    pub admin_session_secret: String,
}

#[derive(Debug, Clone)]
pub struct RuntimeConfigSnapshot {
    pub effective: ServerConfig,
    pub persisted: ServerConfig,
    pub pending_restart_fields: Vec<String>,
}

#[derive(Clone)]
pub struct RuntimeConfig {
    inner: Arc<RwLock<RuntimeConfigState>>,
}

#[derive(Debug, Clone)]
struct RuntimeConfigState {
    effective: ServerConfig,
    persisted: ServerConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredConfig {
    #[serde(default)]
    sync_listen: Vec<String>,
    #[serde(default, skip_serializing)]
    listen: Vec<String>,
    #[serde(default, skip_serializing)]
    bind: Option<String>,
    #[serde(default = "default_db_path")]
    db_path: String,
    #[serde(default)]
    export_dir: String,
    #[serde(default = "default_log_path")]
    log_path: String,
    #[serde(default = "default_log_level")]
    log_level: String,
    #[serde(default)]
    sync_token: String,
    #[serde(default, skip_serializing)]
    token: String,
    #[serde(default)]
    admin_listen: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    admin_password_hash: Option<String>,
    #[serde(default)]
    admin_session_secret: String,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("config parse error: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("config serialize error: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("invalid config: {0}")]
    Invalid(String),
}

pub fn default_config_path() -> Result<PathBuf, ConfigError> {
    let exe_dir = env::current_exe()?
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or(env::current_dir()?);
    Ok(exe_dir.join("sync-server.toml"))
}

pub fn load_or_create_config(
    config_path: &Path,
    overrides: &ConfigOverrides,
) -> Result<ServerConfig, ConfigError> {
    let stored = load_or_create_stored_config(config_path)?;
    let sync_listen = overrides
        .listen
        .clone()
        .unwrap_or_else(|| stored.sync_listen.clone());
    let sync_listen = normalize_listen_addresses("sync_listen", sync_listen)?;

    let sync_token = overrides
        .token
        .clone()
        .unwrap_or_else(|| stored.sync_token.clone())
        .trim()
        .to_string();
    if sync_token.is_empty() {
        return Err(ConfigError::Invalid("sync_token must not be empty".into()));
    }

    // The config file defines the deployment root. Resolving relative paths against the
    // config file location keeps copied Linux bundles self-contained even when the
    // service is started from a different working directory.
    let db_path = overrides
        .db_path
        .as_deref()
        .unwrap_or_else(|| Path::new(&stored.db_path));

    Ok(ServerConfig {
        config_path: config_path.to_path_buf(),
        sync_listen,
        admin_listen: normalize_listen_addresses("admin_listen", stored.admin_listen.clone())?,
        db_path: resolve_path_from_config(config_path, db_path),
        export_dir: resolve_path_from_config(config_path, Path::new(&stored.export_dir)),
        log_path: resolve_path_from_config(config_path, Path::new(&stored.log_path)),
        log_level: stored.log_level.trim().to_string(),
        sync_token,
        admin_password_hash: stored.admin_password_hash.clone(),
        admin_session_secret: stored.admin_session_secret.trim().to_string(),
    })
}

pub fn update_config_file(
    config_path: &Path,
    patch: &ConfigPatch,
) -> Result<ServerConfig, ConfigError> {
    let current = load_or_create_config(config_path, &ConfigOverrides::default())?;
    apply_and_persist_config(&current, patch)
}

pub fn generate_token() -> String {
    Uuid::new_v4().to_string()
}

fn load_or_create_stored_config(config_path: &Path) -> Result<StoredConfig, ConfigError> {
    let (mut stored, mut needs_write) = if config_path.exists() {
        (toml::from_str(&fs::read_to_string(config_path)?)?, false)
    } else {
        (StoredConfig::default(), true)
    };

    needs_write |= normalize_stored_config(&mut stored)?;
    if needs_write {
        write_stored_config(config_path, &stored)?;
    }
    Ok(stored)
}

fn write_stored_config(config_path: &Path, stored: &StoredConfig) -> Result<(), ConfigError> {
    write_config_contents(config_path, &toml::to_string_pretty(stored)?)
}

fn write_server_config(config: &ServerConfig) -> Result<(), ConfigError> {
    let stored = StoredConfig::from_server_config(config);
    write_stored_config(&config.config_path, &stored)
}

fn write_config_contents(config_path: &Path, contents: &str) -> Result<(), ConfigError> {
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = temp_config_path(config_path);
    {
        let mut file = fs::File::create(&temp_path)?;
        use std::io::Write as _;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
    }
    fs::rename(&temp_path, config_path).map_err(|error| {
        let _ = fs::remove_file(&temp_path);
        error
    })?;
    Ok(())
}

fn temp_config_path(config_path: &Path) -> PathBuf {
    let file_name = config_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("sync-server.toml");
    config_path.with_file_name(format!("{file_name}.{}.tmp", Uuid::new_v4()))
}

fn normalize_stored_config(stored: &mut StoredConfig) -> Result<bool, ConfigError> {
    let mut changed = false;

    if stored.sync_listen.is_empty() {
        if !stored.listen.is_empty() {
            stored.sync_listen = stored.listen.clone();
            changed = true;
        } else if let Some(bind) = stored.bind.clone() {
            stored.sync_listen = vec![bind];
            changed = true;
        } else {
            stored.sync_listen = default_sync_listen();
            changed = true;
        }
    }

    if !stored.listen.is_empty() {
        stored.listen.clear();
        changed = true;
    }
    if stored.bind.take().is_some() {
        changed = true;
    }

    let normalized_sync_listen =
        normalize_listen_addresses("sync_listen", stored.sync_listen.clone())?;
    if normalized_sync_listen != stored.sync_listen {
        stored.sync_listen = normalized_sync_listen;
        changed = true;
    }
    if is_legacy_single_stack_default_sync_listen(&stored.sync_listen) {
        stored.sync_listen = default_sync_listen();
        changed = true;
    }

    if stored.admin_listen.is_empty() {
        stored.admin_listen = default_admin_listen();
        changed = true;
    }
    let normalized_admin_listen =
        normalize_listen_addresses("admin_listen", stored.admin_listen.clone())?;
    if normalized_admin_listen != stored.admin_listen {
        stored.admin_listen = normalized_admin_listen;
        changed = true;
    }

    if stored.db_path.trim().is_empty() {
        stored.db_path = default_db_path();
        changed = true;
    }
    if stored.export_dir.trim().is_empty() {
        stored.export_dir = default_export_dir();
        changed = true;
    }
    if stored.log_path.trim().is_empty() {
        stored.log_path = default_log_path();
        changed = true;
    }
    let normalized_log_level = stored.log_level.trim();
    if normalized_log_level.is_empty() {
        stored.log_level = default_log_level();
        changed = true;
    } else if normalized_log_level != stored.log_level {
        stored.log_level = normalized_log_level.to_string();
        changed = true;
    }

    if stored.sync_token.trim().is_empty() {
        if !stored.token.trim().is_empty() {
            stored.sync_token = stored.token.trim().to_string();
            changed = true;
        } else {
            stored.sync_token = generate_token();
            changed = true;
        }
    } else {
        let normalized_sync_token = stored.sync_token.trim().to_string();
        if normalized_sync_token != stored.sync_token {
            stored.sync_token = normalized_sync_token;
            changed = true;
        }
    }
    if !stored.token.is_empty() {
        stored.token.clear();
        changed = true;
    }

    match stored.admin_password_hash.as_ref().map(|value| value.trim()) {
        Some("") => {
            stored.admin_password_hash = None;
            changed = true;
        }
        Some(trimmed) => {
            let normalized = trimmed.to_string();
            if stored.admin_password_hash.as_ref() != Some(&normalized) {
                stored.admin_password_hash = Some(normalized);
                changed = true;
            }
        }
        None => {}
    }

    let normalized_admin_session_secret = stored.admin_session_secret.trim().to_string();
    if normalized_admin_session_secret.is_empty() {
        stored.admin_session_secret = generate_token();
        changed = true;
    } else if normalized_admin_session_secret != stored.admin_session_secret {
        stored.admin_session_secret = normalized_admin_session_secret;
        changed = true;
    }

    if stored.db_path.trim().is_empty() {
        return Err(ConfigError::Invalid("db_path must not be empty".into()));
    }
    if stored.export_dir.trim().is_empty() {
        return Err(ConfigError::Invalid("export_dir must not be empty".into()));
    }
    if stored.log_path.trim().is_empty() {
        return Err(ConfigError::Invalid("log_path must not be empty".into()));
    }
    if stored.log_level.trim().is_empty() {
        return Err(ConfigError::Invalid("log_level must not be empty".into()));
    }
    if stored.sync_token.trim().is_empty() {
        return Err(ConfigError::Invalid("sync_token must not be empty".into()));
    }
    if stored.admin_session_secret.trim().is_empty() {
        return Err(ConfigError::Invalid(
            "admin_session_secret must not be empty".into(),
        ));
    }
    Ok(changed)
}

fn normalize_listen_addresses(
    field_name: &str,
    listen: Vec<String>,
) -> Result<Vec<String>, ConfigError> {
    let mut normalized = Vec::new();
    for address in listen {
        let trimmed = address.trim();
        if trimmed.is_empty() {
            continue;
        }
        trimmed.parse::<SocketAddr>().map_err(|_| {
            ConfigError::Invalid(format!(
                "{field_name} entry must be a valid socket address: {trimmed}"
            ))
        })?;
        if !normalized.iter().any(|existing| existing == trimmed) {
            normalized.push(trimmed.to_string());
        }
    }

    if normalized.is_empty() {
        return Err(ConfigError::Invalid(
            format!("{field_name} must contain at least one address"),
        ));
    }

    Ok(normalized)
}

pub fn apply_config_patch(
    current: &ServerConfig,
    patch: &ConfigPatch,
) -> Result<ServerConfig, ConfigError> {
    let mut updated = current.clone();

    if let Some(listen) = &patch.listen {
        updated.sync_listen = normalize_listen_addresses("sync_listen", listen.clone())?;
    }
    if let Some(admin_listen) = &patch.admin_listen {
        updated.admin_listen = normalize_listen_addresses("admin_listen", admin_listen.clone())?;
    }
    if let Some(db_path) = &patch.db_path {
        updated.db_path = normalize_patch_path(&current.config_path, "db_path", db_path)?;
    }
    if let Some(export_dir) = &patch.export_dir {
        updated.export_dir = normalize_patch_path(&current.config_path, "export_dir", export_dir)?;
    }
    if let Some(log_path) = &patch.log_path {
        updated.log_path = normalize_patch_path(&current.config_path, "log_path", log_path)?;
    }
    if let Some(log_level) = &patch.log_level {
        let trimmed = log_level.trim();
        if trimmed.is_empty() {
            return Err(ConfigError::Invalid("log_level patch must not be empty".into()));
        }
        updated.log_level = trimmed.to_string();
    }
    if let Some(token) = &patch.token {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            return Err(ConfigError::Invalid(
                "sync_token patch must not be empty".into(),
            ));
        }
        updated.sync_token = trimmed.to_string();
    }
    if let Some(admin_password_hash) = &patch.admin_password_hash {
        let trimmed = admin_password_hash.trim();
        if trimmed.is_empty() {
            return Err(ConfigError::Invalid(
                "admin_password_hash patch must not be empty".into(),
            ));
        }
        updated.admin_password_hash = Some(trimmed.to_string());
    }

    Ok(updated)
}

pub fn apply_and_persist_config(
    current: &ServerConfig,
    patch: &ConfigPatch,
) -> Result<ServerConfig, ConfigError> {
    let updated = apply_config_patch(current, patch)?;
    write_server_config(&updated)?;
    Ok(updated)
}

fn normalize_patch_path(
    config_path: &Path,
    field_name: &str,
    path: &Path,
) -> Result<PathBuf, ConfigError> {
    if path.as_os_str().is_empty() {
        return Err(ConfigError::Invalid(format!(
            "{field_name} patch must not be empty"
        )));
    }
    Ok(resolve_path_from_config(config_path, path))
}

fn resolve_path_from_config(config_path: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }

    config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(path)
}

fn default_sync_listen() -> Vec<String> {
    vec!["0.0.0.0:8787".into(), "[::]:8787".into()]
}

fn is_legacy_single_stack_default_sync_listen(sync_listen: &[String]) -> bool {
    matches!(
        sync_listen,
        [address] if address == "0.0.0.0:8787" || address == "[::]:8787"
    )
}

fn default_admin_listen() -> Vec<String> {
    vec!["127.0.0.1:8788".into(), "[::1]:8788".into()]
}

fn default_db_path() -> String {
    "data/floral-sync.sqlite3".into()
}

fn default_export_dir() -> String {
    "exports".into()
}

fn default_log_path() -> String {
    "logs/floral-sync-server.log".into()
}

fn default_log_level() -> String {
    "info".into()
}

impl Default for StoredConfig {
    fn default() -> Self {
        Self {
            sync_listen: default_sync_listen(),
            listen: Vec::new(),
            bind: None,
            db_path: default_db_path(),
            export_dir: default_export_dir(),
            log_path: default_log_path(),
            log_level: default_log_level(),
            sync_token: generate_token(),
            token: String::new(),
            admin_listen: default_admin_listen(),
            admin_password_hash: None,
            admin_session_secret: generate_token(),
        }
    }
}

impl StoredConfig {
    fn from_server_config(config: &ServerConfig) -> Self {
        Self {
            sync_listen: config.sync_listen.clone(),
            listen: Vec::new(),
            bind: None,
            db_path: path_for_storage(&config.config_path, &config.db_path),
            export_dir: path_for_storage(&config.config_path, &config.export_dir),
            log_path: path_for_storage(&config.config_path, &config.log_path),
            log_level: config.log_level.clone(),
            sync_token: config.sync_token.clone(),
            token: String::new(),
            admin_listen: config.admin_listen.clone(),
            admin_password_hash: config.admin_password_hash.clone(),
            admin_session_secret: config.admin_session_secret.clone(),
        }
    }
}

fn path_for_storage(config_path: &Path, path: &Path) -> String {
    let Some(base_dir) = config_path.parent() else {
        return path.to_string_lossy().to_string();
    };
    match path.strip_prefix(base_dir) {
        Ok(relative) if !relative.as_os_str().is_empty() => relative.to_string_lossy().to_string(),
        _ => path.to_string_lossy().to_string(),
    }
}

impl RuntimeConfig {
    pub fn new(initial: ServerConfig) -> Self {
        Self {
            inner: Arc::new(RwLock::new(RuntimeConfigState {
                effective: initial.clone(),
                persisted: initial,
            })),
        }
    }

    pub fn effective_config(&self) -> ServerConfig {
        self.inner
            .read()
            .expect("runtime config lock poisoned")
            .effective
            .clone()
    }

    pub fn persisted_config(&self) -> ServerConfig {
        self.inner
            .read()
            .expect("runtime config lock poisoned")
            .persisted
            .clone()
    }

    pub fn snapshot(&self) -> RuntimeConfigSnapshot {
        let state = self.inner.read().expect("runtime config lock poisoned");
        snapshot_from_state(&state)
    }

    pub fn apply_persisted_config(&self, persisted: ServerConfig) {
        let mut state = self.inner.write().expect("runtime config lock poisoned");
        state.persisted = persisted.clone();
        state.effective.sync_token = persisted.sync_token.clone();
        state.effective.admin_password_hash = persisted.admin_password_hash.clone();
        state.effective.admin_session_secret = persisted.admin_session_secret.clone();
        state.effective.log_level = persisted.log_level.clone();
    }

    pub fn apply_patch(&self, patch: &ConfigPatch) -> Result<RuntimeConfigSnapshot, ConfigError> {
        let mut state = self.inner.write().expect("runtime config lock poisoned");
        let updated = apply_and_persist_config(&state.persisted, patch)?;
        state.persisted = updated.clone();
        state.effective.sync_token = updated.sync_token.clone();
        state.effective.admin_password_hash = updated.admin_password_hash.clone();
        state.effective.admin_session_secret = updated.admin_session_secret.clone();
        state.effective.log_level = updated.log_level.clone();
        Ok(snapshot_from_state(&state))
    }
}

fn pending_restart_fields_for(
    effective: &ServerConfig,
    persisted: &ServerConfig,
) -> Vec<String> {
    let mut fields = Vec::new();
    if effective.sync_listen != persisted.sync_listen {
        fields.push("syncListen".into());
    }
    if effective.admin_listen != persisted.admin_listen {
        fields.push("adminListen".into());
    }
    if effective.db_path != persisted.db_path {
        fields.push("dbPath".into());
    }
    if effective.export_dir != persisted.export_dir {
        fields.push("exportDir".into());
    }
    if effective.log_path != persisted.log_path {
        fields.push("logPath".into());
    }
    fields
}

fn snapshot_from_state(state: &RuntimeConfigState) -> RuntimeConfigSnapshot {
    RuntimeConfigSnapshot {
        effective: state.effective.clone(),
        persisted: state.persisted.clone(),
        pending_restart_fields: pending_restart_fields_for(&state.effective, &state.persisted),
    }
}

#[cfg(test)]
mod tests {
    use super::{load_or_create_config, update_config_file, ConfigOverrides, ConfigPatch};
    use std::fs;

    fn default_admin_listen() -> Vec<&'static str> {
        vec!["127.0.0.1:8788", "[::1]:8788"]
    }

    #[test]
    fn creates_default_config_file_when_missing() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_path = temp.path().join("sync-server.toml");

        let config = load_or_create_config(&config_path, &ConfigOverrides::default())
            .expect("create default config");

        assert!(config_path.exists());
        assert_eq!(config.config_path, config_path);
        assert_eq!(config.sync_listen, vec!["0.0.0.0:8787", "[::]:8787"]);
        assert_eq!(
            config.admin_listen,
            default_admin_listen()
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        );
        assert_eq!(config.db_path, temp.path().join("data").join("floral-sync.sqlite3"));
        assert_eq!(config.export_dir, temp.path().join("exports"));
        assert_eq!(
            config.log_path,
            temp.path().join("logs").join("floral-sync-server.log")
        );
        assert_eq!(config.log_level, "info");
        assert!(!config.sync_token.trim().is_empty());
        assert!(config.admin_password_hash.is_none());
        assert!(!config.admin_session_secret.trim().is_empty());

        let raw = fs::read_to_string(&config_path).expect("read config");
        assert!(raw.contains("sync_listen = ["));
        assert!(raw.contains("admin_listen = ["));
        assert!(raw.contains("sync_token = "));
        assert!(raw.contains("admin_session_secret = "));
    }

    #[test]
    fn resolves_relative_database_path_and_applies_cli_overrides_without_rewriting_file() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("nested");
        fs::create_dir_all(&config_dir).expect("create config dir");
        let config_path = config_dir.join("sync-server.toml");
        fs::write(
            &config_path,
            "sync_listen = [\"0.0.0.0:9000\"]\ndb_path = \"state/sync.sqlite3\"\nsync_token = \"file-token\"\nadmin_listen = [\"127.0.0.1:8788\"]\nadmin_session_secret = \"session-secret\"\n",
        )
        .expect("write config");

        let config = load_or_create_config(
            &config_path,
            &ConfigOverrides {
                listen: Some(vec!["127.0.0.1:9001".into(), "[::1]:9001".into()]),
                db_path: None,
                token: Some("cli-token".into()),
            },
        )
        .expect("load config");

        assert_eq!(config.sync_listen, vec!["127.0.0.1:9001", "[::1]:9001"]);
        assert_eq!(config.sync_token, "cli-token");
        assert_eq!(
            config.db_path,
            config_dir.join("state").join("sync.sqlite3")
        );

        let raw = fs::read_to_string(&config_path).expect("read config");
        assert!(raw.contains("0.0.0.0:9000"));
        assert!(raw.contains("file-token"));
    }

    #[test]
    fn config_set_updates_selected_fields_without_losing_existing_values() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_path = temp.path().join("sync-server.toml");
        let new_db_path = temp.path().join("server-data").join("sync.sqlite3");
        fs::write(
            &config_path,
            "sync_listen = [\"[::]:8787\"]\ndb_path = \"data/floral-sync.sqlite3\"\nsync_token = \"original-token\"\nadmin_listen = [\"127.0.0.1:8788\"]\nadmin_session_secret = \"original-secret\"\n",
        )
        .expect("write config");

        let config = update_config_file(
            &config_path,
            &ConfigPatch {
                listen: Some(vec!["0.0.0.0:9900".into(), "[::]:9900".into()]),
                admin_listen: None,
                db_path: Some(new_db_path.clone()),
                export_dir: None,
                log_path: None,
                log_level: None,
                token: None,
                admin_password_hash: None,
            },
        )
        .expect("update config");

        assert_eq!(config.sync_listen, vec!["0.0.0.0:9900", "[::]:9900"]);
        assert_eq!(config.db_path, new_db_path);
        assert_eq!(config.sync_token, "original-token");

        let raw = fs::read_to_string(&config_path).expect("read config");
        assert!(raw.contains("sync_listen = ["));
        assert!(raw.contains("server-data"));
        assert!(raw.contains("original-token"));
    }

    #[test]
    fn migrates_legacy_listen_field_to_sync_listen() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_path = temp.path().join("sync-server.toml");
        fs::write(
            &config_path,
            "listen = [\"127.0.0.1:8787\"]\ndb_path = \"data/floral-sync.sqlite3\"\ntoken = \"legacy-token\"\nadmin_session_secret = \"legacy-secret\"\n",
        )
        .expect("write legacy config");

        let config = load_or_create_config(&config_path, &ConfigOverrides::default())
            .expect("load migrated config");

        assert_eq!(config.sync_listen, vec!["127.0.0.1:8787"]);
        assert_eq!(config.sync_token, "legacy-token");
        let raw = fs::read_to_string(&config_path).expect("read config");
        assert!(raw.contains("sync_listen = ["));
        assert!(raw.contains("sync_token = "));
        assert!(!raw.lines().any(|line| line.starts_with("listen = ")));
        assert!(!raw.lines().any(|line| line.starts_with("token = ")));
    }

    #[test]
    fn migrates_legacy_bind_field_to_sync_listen() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_path = temp.path().join("sync-server.toml");
        fs::write(
            &config_path,
            "bind = \"[::1]:8787\"\ndb_path = \"data/floral-sync.sqlite3\"\ntoken = \"legacy-token\"\nadmin_session_secret = \"legacy-secret\"\n",
        )
        .expect("write legacy config");

        let config = load_or_create_config(&config_path, &ConfigOverrides::default())
            .expect("load migrated config");

        assert_eq!(config.sync_listen, vec!["[::1]:8787"]);
        let raw = fs::read_to_string(&config_path).expect("read config");
        assert!(raw.contains("sync_listen = ["));
        assert!(!raw.contains("bind = "));
    }

    #[test]
    fn expands_legacy_single_stack_default_sync_listen_to_dual_stack() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_path = temp.path().join("sync-server.toml");
        fs::write(
            &config_path,
            "sync_listen = [\"[::]:8787\"]\ndb_path = \"data/floral-sync.sqlite3\"\nsync_token = \"sync-token\"\nadmin_listen = [\"127.0.0.1:8788\"]\nadmin_session_secret = \"session-secret\"\n",
        )
        .expect("write config");

        let config = load_or_create_config(&config_path, &ConfigOverrides::default())
            .expect("load config");

        assert_eq!(config.sync_listen, vec!["0.0.0.0:8787", "[::]:8787"]);
        let raw = fs::read_to_string(&config_path).expect("read config");
        assert!(raw.contains("0.0.0.0:8787"));
        assert!(raw.contains("[::]:8787"));
    }

    #[test]
    fn keeps_explicit_non_default_single_stack_sync_listen() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_path = temp.path().join("sync-server.toml");
        fs::write(
            &config_path,
            "sync_listen = [\"[::]:9900\"]\ndb_path = \"data/floral-sync.sqlite3\"\nsync_token = \"sync-token\"\nadmin_listen = [\"127.0.0.1:8788\"]\nadmin_session_secret = \"session-secret\"\n",
        )
        .expect("write config");

        let config = load_or_create_config(&config_path, &ConfigOverrides::default())
            .expect("load config");

        assert_eq!(config.sync_listen, vec!["[::]:9900"]);
    }

    #[test]
    fn missing_admin_listen_defaults_to_loopback_admin_addresses() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_path = temp.path().join("sync-server.toml");
        fs::write(
            &config_path,
            "sync_listen = [\"0.0.0.0:8787\"]\ndb_path = \"data/floral-sync.sqlite3\"\nsync_token = \"sync-token\"\nadmin_session_secret = \"session-secret\"\nadmin_password_hash = \"hashed-password\"\n",
        )
        .expect("write config");

        let config = load_or_create_config(&config_path, &ConfigOverrides::default())
            .expect("load config");

        assert_eq!(
            config.admin_listen,
            default_admin_listen()
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        );
        let raw = fs::read_to_string(&config_path).expect("read config");
        assert!(raw.contains("admin_listen = ["));
        assert!(raw.contains("127.0.0.1:8788"));
        assert!(raw.contains("[::1]:8788"));
    }

    #[test]
    fn missing_admin_session_secret_is_generated() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_path = temp.path().join("sync-server.toml");
        fs::write(
            &config_path,
            "sync_listen = [\"0.0.0.0:8787\"]\ndb_path = \"data/floral-sync.sqlite3\"\nsync_token = \"sync-token\"\nadmin_listen = [\"127.0.0.1:8788\"]\nadmin_password_hash = \"hashed-password\"\n",
        )
        .expect("write config");

        let config = load_or_create_config(&config_path, &ConfigOverrides::default())
            .expect("load config");

        assert!(!config.admin_session_secret.trim().is_empty());
        let raw = fs::read_to_string(&config_path).expect("read config");
        assert!(raw.contains("admin_session_secret = "));
    }

    #[test]
    fn missing_admin_password_hash_leaves_bootstrap_mode_enabled() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_path = temp.path().join("sync-server.toml");
        fs::write(
            &config_path,
            "sync_listen = [\"0.0.0.0:8787\"]\ndb_path = \"data/floral-sync.sqlite3\"\nsync_token = \"sync-token\"\nadmin_listen = [\"127.0.0.1:8788\"]\nadmin_session_secret = \"session-secret\"\n",
        )
        .expect("write config");

        let config = load_or_create_config(&config_path, &ConfigOverrides::default())
            .expect("load config");

        assert!(config.admin_password_hash.is_none());
        let raw = fs::read_to_string(&config_path).expect("read config");
        assert!(!raw.contains("admin_password_hash = "));
    }

    #[test]
    fn rejects_invalid_sync_listen_addresses_during_config_load() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_path = temp.path().join("sync-server.toml");
        fs::write(
            &config_path,
            "sync_listen = [\"not-an-address\"]\ndb_path = \"data/floral-sync.sqlite3\"\nsync_token = \"sync-token\"\nadmin_listen = [\"127.0.0.1:8788\"]\nadmin_session_secret = \"session-secret\"\n",
        )
        .expect("write config");

        let error = load_or_create_config(&config_path, &ConfigOverrides::default())
            .expect_err("invalid sync listen should fail");

        assert_eq!(
            error.to_string(),
            "invalid config: sync_listen entry must be a valid socket address: not-an-address"
        );
    }

    #[test]
    fn rejects_invalid_admin_listen_addresses_during_config_load() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_path = temp.path().join("sync-server.toml");
        fs::write(
            &config_path,
            "sync_listen = [\"0.0.0.0:8787\"]\ndb_path = \"data/floral-sync.sqlite3\"\nsync_token = \"sync-token\"\nadmin_listen = [\"nope\"]\nadmin_session_secret = \"session-secret\"\n",
        )
        .expect("write config");

        let error = load_or_create_config(&config_path, &ConfigOverrides::default())
            .expect_err("invalid admin listen should fail");

        assert_eq!(
            error.to_string(),
            "invalid config: admin_listen entry must be a valid socket address: nope"
        );
    }

    #[test]
    fn rejects_whitespace_token_updates_without_rotating_credentials() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_path = temp.path().join("sync-server.toml");
        fs::write(
            &config_path,
            "sync_listen = [\"0.0.0.0:8787\"]\ndb_path = \"data/floral-sync.sqlite3\"\nsync_token = \"original-token\"\nadmin_listen = [\"127.0.0.1:8788\"]\nadmin_session_secret = \"session-secret\"\n",
        )
        .expect("write config");

        let error = update_config_file(
            &config_path,
            &ConfigPatch {
                listen: None,
                admin_listen: None,
                db_path: None,
                export_dir: None,
                log_path: None,
                log_level: None,
                token: Some("   ".into()),
                admin_password_hash: None,
            },
        )
        .expect_err("whitespace token should fail");

        assert_eq!(
            error.to_string(),
            "invalid config: sync_token patch must not be empty"
        );

        let raw = fs::read_to_string(&config_path).expect("read config");
        assert!(raw.contains("sync_token = \"original-token\""));
    }
}
