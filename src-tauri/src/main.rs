#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use chrono::{Datelike, Local, NaiveDate};
use rand::{rngs::OsRng, RngCore};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, State,
};

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const SCHEMA_VERSION: i64 = 9;
const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;

struct AppState {
    root: PathBuf,
    db: Mutex<Option<Connection>>,
    previous_exit_dirty: Mutex<bool>,
    protection_master: Mutex<Option<[u8; 32]>>,
    tasks: TaskRegistry,
}

impl AppState {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            db: Mutex::new(None),
            previous_exit_dirty: Mutex::new(false),
            protection_master: Mutex::new(None),
            tasks: TaskRegistry::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct BackgroundTask {
    id: u64,
    kind: String,
    label: String,
    started_at: String,
}

#[derive(Clone)]
struct TaskRegistry {
    next_id: Arc<AtomicU64>,
    active: Arc<Mutex<HashMap<u64, BackgroundTask>>>,
}

struct TaskLease {
    registry: TaskRegistry,
    id: u64,
}

impl TaskRegistry {
    fn new() -> Self {
        Self {
            next_id: Arc::new(AtomicU64::new(1)),
            active: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn begin(&self, kind: &str, label: &str) -> Result<TaskLease, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let task = BackgroundTask {
            id,
            kind: kind.to_string(),
            label: label.to_string(),
            started_at: now_iso(),
        };
        self.active.lock().map_err(to_error)?.insert(id, task);
        Ok(TaskLease {
            registry: self.clone(),
            id,
        })
    }

    fn snapshot(&self) -> Result<Vec<BackgroundTask>, String> {
        let mut tasks: Vec<BackgroundTask> = self
            .active
            .lock()
            .map_err(to_error)?
            .values()
            .cloned()
            .collect();
        tasks.sort_by(|a, b| {
            a.started_at
                .cmp(&b.started_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(tasks)
    }
}

impl Drop for TaskLease {
    fn drop(&mut self) {
        if let Ok(mut active) = self.registry.active.lock() {
            active.remove(&self.id);
        }
    }
}

#[derive(Debug, Serialize)]
struct BootstrapInfo {
    app_version: String,
    schema_version: i64,
    root: String,
    previous_exit_dirty: bool,
    previous_exit_time: Option<String>,
    startup_integrity: String,
    recovery_log_path: String,
    protection_enabled: bool,
    close_behavior: String,
}

#[derive(Debug, Serialize)]
struct BackupResult {
    path: String,
    checksum: String,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct BackupEntry {
    path: String,
    checksum: String,
    size_bytes: i64,
    schema_version: i64,
    result: String,
    created_at: String,
    exists: bool,
}

#[derive(Debug, Clone, Serialize)]
struct RestoreDrillRecord {
    id: i64,
    backup_path: String,
    backup_checksum: String,
    backup_created_at: Option<String>,
    started_at: String,
    finished_at: Option<String>,
    duration_ms: Option<i64>,
    result: String,
    error_summary: String,
}

#[derive(Debug, Serialize)]
struct OperationalMetric {
    key: String,
    label: String,
    value: Option<f64>,
    target: String,
    status: String,
    unit: String,
    formula: String,
    source: String,
    window: String,
    sample_size: i64,
}

#[derive(Debug, Serialize)]
struct OperationalAssessment {
    generated_at: String,
    window_start: String,
    window_end: String,
    metrics: Vec<OperationalMetric>,
    latest_restore_drill: Option<RestoreDrillRecord>,
    demo_only: bool,
}

#[derive(Debug)]
struct RestoreDrillVerification {
    checksum: String,
    backup_created_at: Option<String>,
    duration_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackupManifestEntry {
    path: String,
    checksum: String,
    size_bytes: i64,
    schema_version: i64,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct CloudBackupFile {
    file_name: String,
    bytes: Vec<u8>,
    checksum: String,
    size_bytes: i64,
    schema_version: i64,
    created_at: String,
}

#[derive(Debug, Default)]
struct CurrentDatabaseSnapshot {
    active: Option<Vec<u8>>,
    encrypted: Option<Vec<u8>>,
}

#[derive(Debug, Deserialize)]
struct ExitState {
    status: String,
    #[serde(default)]
    time: Option<String>,
}

#[derive(Debug)]
struct PreviousExit {
    dirty: bool,
    time: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ProtectionConfig {
    version: u8,
    salt_b64: String,
    password_wrapped_b64: String,
    recovery_wrapped_b64: String,
}

const ENCRYPTED_MAGIC: &[u8] = b"ERPENC1";
const MAX_CLOUD_BACKUP_BYTES: usize = 200 * 1024 * 1024;

#[cfg(windows)]
#[link(name = "Kernel32")]
extern "system" {
    fn ReplaceFileW(
        replaced_file_name: *const u16,
        replacement_file_name: *const u16,
        backup_file_name: *const u16,
        replace_flags: u32,
        exclude: *mut std::ffi::c_void,
        reserved: *mut std::ffi::c_void,
    ) -> i32;
}

fn to_error<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
}

fn file_operation_error(path: &Path, error: std::io::Error) -> String {
    let sharing_violation = matches!(error.raw_os_error(), Some(5 | 32 | 33));
    if sharing_violation
        || matches!(
            error.kind(),
            std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::WouldBlock
        )
    {
        format!(
            "文件正在被其他程序占用或无访问权限，请关闭占用程序后重试：{}",
            path.display()
        )
    } else if error.kind() == std::io::ErrorKind::NotFound {
        format!("文件不存在：{}", path.display())
    } else {
        format!("文件操作失败（{}）：{}", path.display(), error)
    }
}

fn csv_file_error(path: &Path, error: csv::Error) -> String {
    if error.is_io_error() {
        format!(
            "文件正在被其他程序占用或无写入权限，请关闭占用程序后重试：{}",
            path.display()
        )
    } else {
        format!("CSV 文件操作失败（{}）：{}", path.display(), error)
    }
}

fn database_file_error(path: &Path, error: impl std::fmt::Display) -> String {
    let message = error.to_string();
    if message.to_lowercase().contains("locked") || message.to_lowercase().contains("busy") {
        format!(
            "账套文件正在被其他程序占用，请关闭占用程序后重试：{}",
            path.display()
        )
    } else {
        message
    }
}

fn now_iso() -> String {
    Local::now().to_rfc3339()
}

fn today() -> String {
    Local::now().date_naive().to_string()
}

#[cfg(windows)]
fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(windows)]
fn replace_file_atomically(temp: &Path, target: &Path) -> Result<(), String> {
    if !target.exists() {
        return fs::rename(temp, target).map_err(|error| file_operation_error(target, error));
    }
    let target_wide = wide_path(target);
    let temp_wide = wide_path(temp);
    let replaced = unsafe {
        ReplaceFileW(
            target_wide.as_ptr(),
            temp_wide.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if replaced == 0 {
        return Err(format!(
            "原子替换文件失败：{}",
            file_operation_error(target, std::io::Error::last_os_error())
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_file_atomically(temp: &Path, target: &Path) -> Result<(), String> {
    fs::rename(temp, target).map_err(|error| file_operation_error(target, error))
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0_u8; N];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

fn derive_password_key(secret: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    let params = Params::new(19_456, 3, 1, Some(32)).map_err(to_error)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0_u8; 32];
    argon
        .hash_password_into(secret.as_bytes(), salt, &mut key)
        .map_err(to_error)?;
    Ok(key)
}

fn sha_key(secret: &str) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(secret.as_bytes());
    digest.finalize().into()
}

fn encrypt_bytes(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(to_error)?;
    let nonce_bytes = random_bytes::<12>();
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext)
        .map_err(to_error)?;
    let mut output =
        Vec::with_capacity(ENCRYPTED_MAGIC.len() + nonce_bytes.len() + ciphertext.len());
    output.extend_from_slice(ENCRYPTED_MAGIC);
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

fn write_bytes_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "目标文件目录无效".to_string())?;
    fs::create_dir_all(parent).map_err(|error| file_operation_error(parent, error))?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "目标文件名无效".to_string())?;
    let temp = parent.join(format!(
        ".{file_name}.{}.tmp",
        hex::encode(random_bytes::<8>())
    ));
    let result = (|| {
        let mut file =
            fs::File::create(&temp).map_err(|error| file_operation_error(&temp, error))?;
        file.write_all(bytes)
            .map_err(|error| file_operation_error(&temp, error))?;
        file.sync_all()
            .map_err(|error| file_operation_error(&temp, error))?;
        drop(file);
        replace_file_atomically(&temp, path)
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    Ok(())
}

fn remove_file_if_exists(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(file_operation_error(path, error)),
    }
}

fn decrypt_bytes(key: &[u8; 32], encrypted: &[u8]) -> Result<Vec<u8>, String> {
    let header_len = ENCRYPTED_MAGIC.len() + 12;
    if encrypted.len() <= header_len || &encrypted[..ENCRYPTED_MAGIC.len()] != ENCRYPTED_MAGIC {
        return Err("加密文件格式无效".into());
    }
    let nonce = Nonce::from_slice(&encrypted[ENCRYPTED_MAGIC.len()..header_len]);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(to_error)?;
    cipher
        .decrypt(nonce, &encrypted[header_len..])
        .map_err(|_| "口令或恢复密钥不正确，无法解密数据".to_string())
}

fn protection_config_path(root: &Path) -> PathBuf {
    root.join("config").join("protection.json")
}

fn load_protection_config(root: &Path) -> Result<Option<ProtectionConfig>, String> {
    let path = protection_config_path(root);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read(&path).map_err(to_error)?;
    serde_json::from_slice(&raw).map(Some).map_err(to_error)
}

fn save_protection_config(root: &Path, config: &ProtectionConfig) -> Result<(), String> {
    let path = protection_config_path(root);
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, serde_json::to_vec_pretty(config).map_err(to_error)?).map_err(to_error)?;
    replace_file_atomically(&temp, &path)
}

fn wrap_master(key: &[u8; 32], master: &[u8; 32]) -> Result<String, String> {
    Ok(B64.encode(encrypt_bytes(key, master)?))
}

fn unwrap_master(key: &[u8; 32], wrapped_b64: &str) -> Result<[u8; 32], String> {
    let encrypted = B64.decode(wrapped_b64).map_err(to_error)?;
    let plain = decrypt_bytes(key, &encrypted)?;
    if plain.len() != 32 {
        return Err("主密钥长度无效".into());
    }
    let mut master = [0_u8; 32];
    master.copy_from_slice(&plain);
    Ok(master)
}

fn resolve_master(config: &ProtectionConfig, secret: &str) -> Result<[u8; 32], String> {
    let salt = B64.decode(&config.salt_b64).map_err(to_error)?;
    if let Ok(key) = derive_password_key(secret, &salt) {
        if let Ok(master) = unwrap_master(&key, &config.password_wrapped_b64) {
            return Ok(master);
        }
    }
    let recovery_key = sha_key(secret);
    unwrap_master(&recovery_key, &config.recovery_wrapped_b64)
}

fn encrypted_db_path(root: &Path) -> PathBuf {
    root.join("data").join("erp.db.enc")
}

fn active_db_path(root: &Path) -> PathBuf {
    root.join("data").join("erp.db")
}

fn backup_manifest_path(root: &Path) -> PathBuf {
    root.join("backup").join("manifest.json")
}

fn load_backup_manifest(root: &Path) -> Result<Vec<BackupManifestEntry>, String> {
    let path = backup_manifest_path(root);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = fs::read(&path).map_err(|error| file_operation_error(&path, error))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("备份清单格式无效：{error}"))
}

fn save_backup_manifest(root: &Path, entries: &[BackupManifestEntry]) -> Result<(), String> {
    let path = backup_manifest_path(root);
    let bytes = serde_json::to_vec_pretty(entries).map_err(to_error)?;
    write_bytes_atomically(&path, &bytes).map_err(|error| format!("备份清单写入失败：{error}"))
}

fn register_backup_manifest(root: &Path, entry: BackupManifestEntry) -> Result<(), String> {
    let mut entries = load_backup_manifest(root)?;
    entries.retain(|item| item.path != entry.path && Path::new(&item.path).exists());
    entries.push(entry);
    entries.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| b.path.cmp(&a.path))
    });
    save_backup_manifest(root, &entries)
}

fn remove_missing_backup_manifest_entries(root: &Path) -> Result<(), String> {
    let entries = load_backup_manifest(root)?;
    let retained: Vec<_> = entries
        .into_iter()
        .filter(|entry| Path::new(&entry.path).exists())
        .collect();
    save_backup_manifest(root, &retained)
}

fn decrypt_database_if_needed(root: &Path, master: Option<&[u8; 32]>) -> Result<(), String> {
    let encrypted_path = encrypted_db_path(root);
    if !encrypted_path.exists() {
        return Ok(());
    }
    let master = master.ok_or_else(|| "发现加密数据库但无法解锁".to_string())?;
    let encrypted = fs::read(&encrypted_path).map_err(to_error)?;
    let plain = decrypt_bytes(master, &encrypted)
        .map_err(|error| format!("主库加密文件损坏或无法解密：{error}"))?;
    let temp = active_db_path(root).with_extension("restore.tmp.db");
    remove_sqlite_artifacts(&temp);
    if let Err(error) = write_bytes_atomically(&temp, &plain) {
        remove_sqlite_artifacts(&temp);
        return Err(format!("解密主库临时文件写入失败：{error}"));
    }
    if let Err(error) = replace_file_atomically(&temp, &active_db_path(root)) {
        remove_sqlite_artifacts(&temp);
        return Err(format!("解密主库替换失败：{error}"));
    }
    Ok(())
}

fn sync_encrypted_database_shadow(root: &Path, master: &[u8; 32]) -> Result<(), String> {
    let plain_path = active_db_path(root);
    let plain = fs::read(&plain_path).map_err(|error| {
        format!(
            "加密数据库 shadow 同步前读取主库失败：{}",
            file_operation_error(&plain_path, error)
        )
    })?;
    let encrypted = encrypt_bytes(master, &plain)?;
    let target = encrypted_db_path(root);
    let temp = target.with_extension("enc.sync.tmp");
    remove_file_if_exists(&temp)?;
    if let Err(error) = write_bytes_atomically(&temp, &encrypted) {
        let _ = remove_file_if_exists(&temp);
        return Err(format!("加密数据库 shadow 临时文件写入失败：{error}"));
    }
    if let Err(error) = replace_file_atomically(&temp, &target) {
        let _ = remove_file_if_exists(&temp);
        return Err(format!("加密数据库 shadow 同步失败：{error}"));
    }
    Ok(())
}

fn resolve_startup_master(root: &Path, secret: Option<&str>) -> Result<Option<[u8; 32]>, String> {
    match load_protection_config(root)? {
        Some(config) => {
            let secret = secret.ok_or_else(|| "PROTECTED_LOCKED".to_string())?;
            resolve_master(&config, secret).map(Some)
        }
        None if encrypted_db_path(root).exists() => Err("发现加密数据库但缺少保护配置".to_string()),
        None => Ok(None),
    }
}

fn encrypt_database_at_rest(root: &Path, master: &[u8; 32]) -> Result<(), String> {
    let plain_path = active_db_path(root);
    let plain = fs::read(&plain_path).map_err(to_error)?;
    let encrypted = encrypt_bytes(master, &plain)?;
    let target = encrypted_db_path(root);
    let temp = target.with_extension("enc.tmp");
    fs::write(&temp, encrypted).map_err(to_error)?;
    replace_file_atomically(&temp, &target)?;
    fs::remove_file(&plain_path).map_err(to_error)?;
    for suffix in ["-wal", "-shm"] {
        let _ = fs::remove_file(root.join("data").join(format!("erp.db{suffix}")));
    }
    Ok(())
}

fn ensure_dirs(root: &Path) -> Result<(), String> {
    for dir in [
        "data", "backup", "export", "files", "logs", "state", "config",
    ] {
        fs::create_dir_all(root.join(dir)).map_err(to_error)?;
    }
    Ok(())
}

fn exit_state_path(root: &Path) -> PathBuf {
    root.join("state").join("last_exit.json")
}

fn ui_ready_path(root: &Path) -> PathBuf {
    root.join("state").join("ui_ready.json")
}

fn has_existing_database(root: &Path) -> bool {
    active_db_path(root).exists() || encrypted_db_path(root).exists()
}

fn read_previous_exit(root: &Path) -> PreviousExit {
    let state = fs::read_to_string(exit_state_path(root))
        .ok()
        .and_then(|raw| serde_json::from_str::<ExitState>(&raw).ok());
    match state {
        Some(state) => PreviousExit {
            dirty: state.status != "clean",
            time: state.time,
        },
        None => PreviousExit {
            dirty: has_existing_database(root),
            time: None,
        },
    }
}

fn write_exit_state(root: &Path, status: &str, backup_path: Option<&str>) -> Result<(), String> {
    let path = exit_state_path(root);
    let temp = root.join("state").join("last_exit.json.tmp");
    let payload = json!({ "status": status, "time": now_iso(), "version": APP_VERSION, "backup_path": backup_path });
    fs::write(
        &temp,
        serde_json::to_vec_pretty(&payload).map_err(to_error)?,
    )
    .map_err(to_error)?;
    replace_file_atomically(&temp, &path)
}

fn append_runtime_log(root: &Path, event: &str, details: Value) -> Result<(), String> {
    let path = root
        .join("logs")
        .join(format!("erp-{}.log", Local::now().format("%Y%m%d")));
    let entry = json!({ "time": now_iso(), "event": event, "details": details });
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(to_error)?;
    writeln!(file, "{}", serde_json::to_string(&entry).map_err(to_error)?).map_err(to_error)
}

fn mark_startup_in_progress(
    root: &Path,
    secret: Option<&str>,
) -> Result<(PreviousExit, Option<[u8; 32]>), String> {
    let previous_exit = read_previous_exit(root);
    let master = resolve_startup_master(root, secret)?;
    write_exit_state(root, "dirty", None)?;
    Ok((previous_exit, master))
}

fn open_database(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(to_error)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(to_error)?;
    conn.execute_batch(
        "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;",
    )
    .map_err(to_error)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS dicts (
            id INTEGER PRIMARY KEY,
            type TEXT NOT NULL,
            value TEXT NOT NULL,
            label TEXT NOT NULL,
            sort_order INTEGER NOT NULL DEFAULT 0,
            enabled INTEGER NOT NULL DEFAULT 1,
            UNIQUE(type, value)
        );
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS customers (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            main_host TEXT NOT NULL DEFAULT '',
            direction TEXT NOT NULL DEFAULT '',
            material_system TEXT NOT NULL DEFAULT '',
            stage TEXT NOT NULL DEFAULT 'lead',
            next_follow_date TEXT,
            notes TEXT NOT NULL DEFAULT '',
            archived_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            UNIQUE(name, main_host)
        );
        CREATE TABLE IF NOT EXISTS contacts (
            id INTEGER PRIMARY KEY,
            customer_id INTEGER NOT NULL REFERENCES customers(id) ON DELETE RESTRICT,
            name TEXT NOT NULL,
            role TEXT NOT NULL DEFAULT '',
            phone TEXT NOT NULL DEFAULT '',
            email TEXT NOT NULL DEFAULT '',
            preferred_channel TEXT NOT NULL DEFAULT '',
            archived_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS projects (
            id INTEGER PRIMARY KEY,
            customer_id INTEGER NOT NULL REFERENCES customers(id) ON DELETE RESTRICT,
            name TEXT NOT NULL,
            main_host TEXT NOT NULL DEFAULT '',
            part_name TEXT NOT NULL DEFAULT '',
            expected_volume_grams INTEGER NOT NULL DEFAULT 0,
            stage TEXT NOT NULL DEFAULT 'lead',
            next_follow_date TEXT,
            archived_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS suppliers (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            contact TEXT NOT NULL DEFAULT '',
            price_ref_cents INTEGER NOT NULL DEFAULT 0,
            archived_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS materials (
            id INTEGER PRIMARY KEY,
            code TEXT NOT NULL UNIQUE,
            base_resin TEXT NOT NULL,
            modification TEXT NOT NULL DEFAULT '',
            mi TEXT NOT NULL DEFAULT '',
            impact TEXT NOT NULL DEFAULT '',
            hdt TEXT NOT NULL DEFAULT '',
            density TEXT NOT NULL DEFAULT '',
            supplier_id INTEGER REFERENCES suppliers(id) ON DELETE RESTRICT,
            cost_cents INTEGER NOT NULL DEFAULT 0,
            cost_date TEXT,
            archived_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS samples (
            id INTEGER PRIMARY KEY,
            code TEXT NOT NULL UNIQUE,
            customer_id INTEGER NOT NULL REFERENCES customers(id) ON DELETE RESTRICT,
            material_id INTEGER NOT NULL REFERENCES materials(id) ON DELETE RESTRICT,
            batch TEXT NOT NULL DEFAULT '',
            weight_grams INTEGER NOT NULL DEFAULT 0,
            sent_date TEXT,
            test_items TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'pending_send',
            fail_reason TEXT NOT NULL DEFAULT '',
            archived_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS sample_tests (
            id INTEGER PRIMARY KEY,
            sample_id INTEGER NOT NULL REFERENCES samples(id) ON DELETE RESTRICT,
            test_date TEXT NOT NULL,
            test_item TEXT NOT NULL,
            result TEXT NOT NULL DEFAULT '',
            conclusion TEXT NOT NULL DEFAULT '',
            next_action TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS contracts (
            id INTEGER PRIMARY KEY,
            no TEXT NOT NULL UNIQUE,
            seller_name TEXT NOT NULL DEFAULT '',
            buyer_name TEXT NOT NULL DEFAULT '',
            customer_id INTEGER REFERENCES customers(id) ON DELETE RESTRICT,
            execution_place TEXT NOT NULL DEFAULT '',
            contract_date TEXT,
            settlement_method TEXT NOT NULL DEFAULT '',
            packaging TEXT NOT NULL DEFAULT '',
            terms TEXT NOT NULL DEFAULT '',
            seller_address TEXT NOT NULL DEFAULT '',
            seller_legal_representative TEXT NOT NULL DEFAULT '',
            seller_agent TEXT NOT NULL DEFAULT '',
            seller_phone TEXT NOT NULL DEFAULT '',
            seller_fax TEXT NOT NULL DEFAULT '',
            seller_bank TEXT NOT NULL DEFAULT '',
            seller_account TEXT NOT NULL DEFAULT '',
            buyer_address TEXT NOT NULL DEFAULT '',
            buyer_legal_representative TEXT NOT NULL DEFAULT '',
            buyer_agent TEXT NOT NULL DEFAULT '',
            buyer_phone TEXT NOT NULL DEFAULT '',
            buyer_fax TEXT NOT NULL DEFAULT '',
            buyer_bank TEXT NOT NULL DEFAULT '',
            buyer_account TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'draft',
            archived_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS contract_items (
            id INTEGER PRIMARY KEY,
            contract_id INTEGER NOT NULL REFERENCES contracts(id) ON DELETE RESTRICT,
            material_id INTEGER REFERENCES materials(id) ON DELETE RESTRICT,
            material_name TEXT NOT NULL DEFAULT '',
            model TEXT NOT NULL DEFAULT '',
            manufacturer TEXT NOT NULL DEFAULT '',
            qty_grams INTEGER NOT NULL DEFAULT 0,
            unit_price_cents INTEGER NOT NULL DEFAULT 0,
            amount_cents INTEGER NOT NULL DEFAULT 0,
            note TEXT NOT NULL DEFAULT ''
        );
        CREATE TABLE IF NOT EXISTS quotations (
            id INTEGER PRIMARY KEY,
            no TEXT NOT NULL UNIQUE,
            customer_id INTEGER NOT NULL REFERENCES customers(id) ON DELETE RESTRICT,
            material_id INTEGER NOT NULL REFERENCES materials(id) ON DELETE RESTRICT,
            price_cents INTEGER NOT NULL CHECK(price_cents > 0),
            moq_grams INTEGER NOT NULL DEFAULT 0,
            freight TEXT NOT NULL DEFAULT '',
            valid_until TEXT,
            seller_name TEXT NOT NULL DEFAULT '',
            recipient_name TEXT NOT NULL DEFAULT '',
            sender_name TEXT NOT NULL DEFAULT '',
            recipient_contact TEXT NOT NULL DEFAULT '',
            recipient_fax TEXT NOT NULL DEFAULT '',
            cc TEXT NOT NULL DEFAULT '',
            page_count INTEGER NOT NULL DEFAULT 1,
            request_review INTEGER NOT NULL DEFAULT 1,
            request_comment INTEGER NOT NULL DEFAULT 1,
            quote_date TEXT,
            subject TEXT NOT NULL DEFAULT '',
            price_note TEXT NOT NULL DEFAULT '',
            adjustment_note TEXT NOT NULL DEFAULT '',
            footer_address TEXT NOT NULL DEFAULT '',
            footer_phone TEXT NOT NULL DEFAULT '',
            footer_fax TEXT NOT NULL DEFAULT '',
            footer_email TEXT NOT NULL DEFAULT '',
            version INTEGER NOT NULL DEFAULT 1,
            status TEXT NOT NULL DEFAULT 'draft',
            archived_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS quotation_items (
            id INTEGER PRIMARY KEY,
            quotation_id INTEGER NOT NULL REFERENCES quotations(id) ON DELETE RESTRICT,
            material_id INTEGER NOT NULL REFERENCES materials(id) ON DELETE RESTRICT,
            batch TEXT NOT NULL DEFAULT '',
            material_category TEXT NOT NULL DEFAULT '',
            material_grade TEXT NOT NULL DEFAULT '',
            manufacturer TEXT NOT NULL DEFAULT '',
            note TEXT NOT NULL DEFAULT '',
            qty_grams INTEGER NOT NULL DEFAULT 0,
            unit_price_cents INTEGER NOT NULL DEFAULT 0,
            amount_cents INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS orders (
            id INTEGER PRIMARY KEY,
            no TEXT NOT NULL UNIQUE,
            customer_id INTEGER NOT NULL REFERENCES customers(id) ON DELETE RESTRICT,
            material_id INTEGER NOT NULL REFERENCES materials(id) ON DELETE RESTRICT,
            qty_grams INTEGER NOT NULL CHECK(qty_grams > 0),
            price_cents INTEGER NOT NULL CHECK(price_cents > 0),
            delivery_date TEXT,
            status TEXT NOT NULL DEFAULT 'pending_confirm',
            sign_status TEXT NOT NULL DEFAULT 'pending',
            quotation_id INTEGER REFERENCES quotations(id) ON DELETE RESTRICT,
            archived_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS order_items (
            id INTEGER PRIMARY KEY,
            order_id INTEGER NOT NULL REFERENCES orders(id) ON DELETE RESTRICT,
            material_id INTEGER NOT NULL REFERENCES materials(id) ON DELETE RESTRICT,
            batch TEXT NOT NULL DEFAULT '',
            qty_grams INTEGER NOT NULL,
            unit_price_cents INTEGER NOT NULL,
            amount_cents INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS deliveries (
            id INTEGER PRIMARY KEY,
            no TEXT NOT NULL UNIQUE,
            order_id INTEGER NOT NULL REFERENCES orders(id) ON DELETE RESTRICT,
            sent_date TEXT,
            sign_date TEXT,
            archived_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS delivery_items (
            id INTEGER PRIMARY KEY,
            delivery_id INTEGER NOT NULL REFERENCES deliveries(id) ON DELETE RESTRICT,
            order_item_id INTEGER REFERENCES order_items(id) ON DELETE RESTRICT,
            batch TEXT NOT NULL DEFAULT '',
            qty_grams INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS statements (
            id INTEGER PRIMARY KEY,
            no TEXT NOT NULL UNIQUE,
            customer_id INTEGER NOT NULL REFERENCES customers(id) ON DELETE RESTRICT,
            period_start TEXT NOT NULL,
            period_end TEXT NOT NULL,
            total_cents INTEGER NOT NULL DEFAULT 0,
            received_cents INTEGER NOT NULL DEFAULT 0,
            unpaid_cents INTEGER NOT NULL DEFAULT 0,
            account_days INTEGER NOT NULL DEFAULT 0,
            aging_days INTEGER NOT NULL DEFAULT 0,
            promise_date TEXT,
            pay_status TEXT NOT NULL DEFAULT 'not_due',
            archived_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS statement_items (
            id INTEGER PRIMARY KEY,
            statement_id INTEGER NOT NULL REFERENCES statements(id) ON DELETE RESTRICT,
            order_id INTEGER NOT NULL REFERENCES orders(id) ON DELETE RESTRICT,
            allocated_amount_cents INTEGER NOT NULL DEFAULT 0,
            UNIQUE(statement_id, order_id)
        );
        CREATE TABLE IF NOT EXISTS payments (
            id INTEGER PRIMARY KEY,
            statement_id INTEGER NOT NULL REFERENCES statements(id) ON DELETE RESTRICT,
            amount_cents INTEGER NOT NULL CHECK(amount_cents > 0),
            pay_date TEXT NOT NULL,
            method TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS payment_allocations (
            id INTEGER PRIMARY KEY,
            payment_id INTEGER NOT NULL REFERENCES payments(id) ON DELETE RESTRICT,
            statement_id INTEGER NOT NULL REFERENCES statements(id) ON DELETE RESTRICT,
            allocated_amount_cents INTEGER NOT NULL CHECK(allocated_amount_cents > 0),
            created_at TEXT NOT NULL,
            UNIQUE(payment_id, statement_id)
        );
        CREATE TABLE IF NOT EXISTS cost_history (
            id INTEGER PRIMARY KEY,
            material_id INTEGER NOT NULL REFERENCES materials(id) ON DELETE RESTRICT,
            cost_cents INTEGER NOT NULL CHECK(cost_cents >= 0),
            cost_date TEXT,
            source TEXT NOT NULL DEFAULT 'manual',
            note TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS purchases (
            id INTEGER PRIMARY KEY,
            no TEXT NOT NULL UNIQUE,
            supplier_id INTEGER NOT NULL REFERENCES suppliers(id) ON DELETE RESTRICT,
            material_id INTEGER NOT NULL REFERENCES materials(id) ON DELETE RESTRICT,
            qty_grams INTEGER NOT NULL CHECK(qty_grams > 0),
            status TEXT NOT NULL DEFAULT 'pending_quote',
            eta TEXT,
            archived_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS inventory (
            id INTEGER PRIMARY KEY,
            material_id INTEGER NOT NULL UNIQUE REFERENCES materials(id) ON DELETE RESTRICT,
            opening_grams INTEGER NOT NULL DEFAULT 0,
            on_hand_grams INTEGER NOT NULL DEFAULT 0,
            in_transit_grams INTEGER NOT NULL DEFAULT 0,
            safety_grams INTEGER NOT NULL DEFAULT 0,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS inventory_movements (
            id INTEGER PRIMARY KEY,
            material_id INTEGER NOT NULL REFERENCES materials(id) ON DELETE RESTRICT,
            movement_type TEXT NOT NULL,
            qty_grams INTEGER NOT NULL,
            batch TEXT NOT NULL DEFAULT '',
            warehouse TEXT NOT NULL DEFAULT '',
            unit_cost_cents INTEGER NOT NULL DEFAULT 0,
            source_id INTEGER,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS stock_reservations (
            id INTEGER PRIMARY KEY,
            material_id INTEGER NOT NULL REFERENCES materials(id) ON DELETE RESTRICT,
            order_id INTEGER REFERENCES orders(id) ON DELETE RESTRICT,
            reserved_grams INTEGER NOT NULL,
            batch TEXT NOT NULL DEFAULT '',
            warehouse TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS follow_ups (
            id INTEGER PRIMARY KEY,
            customer_id INTEGER NOT NULL REFERENCES customers(id) ON DELETE RESTRICT,
            content TEXT NOT NULL,
            follow_date TEXT,
            next_date TEXT,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS attachments (
            id INTEGER PRIMARY KEY,
            object_type TEXT NOT NULL,
            object_id INTEGER NOT NULL,
            relative_path TEXT NOT NULL,
            hash TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS audit_logs (
            id INTEGER PRIMARY KEY,
            object_type TEXT NOT NULL,
            object_id INTEGER NOT NULL,
            event_type TEXT NOT NULL,
            before_json TEXT,
            after_json TEXT,
            reason TEXT,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS sync_events (
            id INTEGER PRIMARY KEY,
            event_id TEXT NOT NULL UNIQUE,
            entity_type TEXT NOT NULL,
            entity_id INTEGER NOT NULL,
            operation TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            synced_at TEXT,
            retry_count INTEGER NOT NULL DEFAULT 0,
            last_error TEXT NOT NULL DEFAULT ''
        );
        CREATE TABLE IF NOT EXISTS import_batches (
            id INTEGER PRIMARY KEY,
            batch_id TEXT NOT NULL UNIQUE,
            entity TEXT NOT NULL,
            source_file TEXT NOT NULL,
            source_checksum TEXT NOT NULL,
            inserted_count INTEGER NOT NULL DEFAULT 0,
            skipped_count INTEGER NOT NULL DEFAULT 0,
            failed_count INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS backup_records (
            id INTEGER PRIMARY KEY,
            path TEXT NOT NULL,
            checksum TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            schema_version INTEGER NOT NULL,
            result TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS restore_drills (
            id INTEGER PRIMARY KEY,
            backup_path TEXT NOT NULL,
            backup_checksum TEXT NOT NULL,
            backup_created_at TEXT,
            started_at TEXT NOT NULL,
            finished_at TEXT,
            duration_ms INTEGER,
            result TEXT NOT NULL,
            error_summary TEXT NOT NULL DEFAULT ''
        );
        CREATE INDEX IF NOT EXISTS idx_customers_follow_date ON customers(next_follow_date);
        CREATE INDEX IF NOT EXISTS idx_orders_delivery_date ON orders(delivery_date);
        CREATE INDEX IF NOT EXISTS idx_statements_promise_date ON statements(promise_date);
        CREATE INDEX IF NOT EXISTS idx_audit_object ON audit_logs(object_type, object_id);
        CREATE INDEX IF NOT EXISTS idx_payment_allocations_statement ON payment_allocations(statement_id);
        CREATE INDEX IF NOT EXISTS idx_inventory_movements_material ON inventory_movements(material_id, created_at DESC, id DESC);
        CREATE INDEX IF NOT EXISTS idx_stock_reservations_material ON stock_reservations(material_id, status);
        CREATE INDEX IF NOT EXISTS idx_cost_history_material ON cost_history(material_id, cost_date DESC, id DESC);
        CREATE INDEX IF NOT EXISTS idx_attachments_object ON attachments(object_type, object_id, created_at DESC, id DESC);
        CREATE INDEX IF NOT EXISTS idx_contracts_updated ON contracts(updated_at DESC, id DESC);
        CREATE INDEX IF NOT EXISTS idx_contract_items_contract ON contract_items(contract_id, id);
        CREATE INDEX IF NOT EXISTS idx_restore_drills_started ON restore_drills(started_at DESC, id DESC);
        CREATE INDEX IF NOT EXISTS idx_sync_events_pending ON sync_events(synced_at, id);
        INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES (1, datetime('now'));
        INSERT OR IGNORE INTO dicts(type, value, label, sort_order) VALUES
            ('customer_stage','lead','潜在客户',10),('customer_stage','contacted','初步接触',20),('customer_stage','sampling','送样测试',30),('customer_stage','quoting','报价谈判',40),('customer_stage','won','定点量产',50),('customer_stage','paused_lost','暂停 / 流失',60),
            ('sample_status','pending_send','待送样',10),('sample_status','sent','已送样',20),('sample_status','testing','测试中',30),('sample_status','passed','通过',40),('sample_status','failed','不通过',50),('sample_status','retest','待复测',60),
            ('order_status','pending_confirm','待确认',10),('order_status','preparing','备货中',20),('order_status','delivered','已送货',30),('order_status','signed','已回签',40),('order_status','completed','已完成',50),('order_status','cancelled','已取消',60),
            ('pay_status','not_due','未到期',10),('pay_status','near_due','临近到期',20),('pay_status','overdue','已逾期',30),('pay_status','settled','已结清',40),
            ('purchase_status','pending_quote','待询价',10),('purchase_status','ordered','已下单',20),('purchase_status','in_transit','在途',30),('purchase_status','received','已入库',40),('purchase_status','closed','关闭',50),('purchase_status','cancelled','已取消',60);
        INSERT OR IGNORE INTO settings(key, value, updated_at) VALUES ('backup_retention','10',datetime('now')),('reminder_days','3',datetime('now')),('protection_enabled','0',datetime('now')),('close_behavior','minimize_to_tray',datetime('now')),('backup_schedule_enabled','1',datetime('now')),('backup_schedule_days','1',datetime('now')),('cloud_sync_enabled','0',datetime('now'));
        "#,
    ).map_err(to_error)?;
    let follow_up_migration_applied: i64 = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=2)",
            [],
            |row| row.get(0),
        )
        .map_err(to_error)?;
    if follow_up_migration_applied == 0 {
        conn.execute_batch(
            r#"
            CREATE INDEX IF NOT EXISTS idx_follow_ups_customer_date ON follow_ups(customer_id, follow_date DESC, id DESC);
            CREATE INDEX IF NOT EXISTS idx_sample_tests_sample_date ON sample_tests(sample_id, test_date DESC, id DESC);
            INSERT INTO follow_ups(customer_id, content, follow_date, next_date, created_at)
            SELECT c.id, '历史迁移：保留原下次跟进日期。', date('now'), c.next_follow_date, datetime('now')
            FROM customers c
            WHERE c.next_follow_date IS NOT NULL
              AND NOT EXISTS (SELECT 1 FROM follow_ups f WHERE f.customer_id = c.id);
            INSERT INTO schema_migrations(version, applied_at) VALUES (2, datetime('now'));
            "#,
        ).map_err(to_error)?;
    }
    let v3_applied: i64 = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=3)",
            [],
            |row| row.get(0),
        )
        .map_err(to_error)?;
    if v3_applied == 0 {
        ensure_column(
            conn,
            "inventory",
            "opening_grams",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_column(
            conn,
            "inventory_movements",
            "warehouse",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(
            conn,
            "inventory_movements",
            "unit_cost_cents",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_column(
            conn,
            "stock_reservations",
            "batch",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(
            conn,
            "stock_reservations",
            "warehouse",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS payment_allocations (
                id INTEGER PRIMARY KEY,
                payment_id INTEGER NOT NULL REFERENCES payments(id) ON DELETE RESTRICT,
                statement_id INTEGER NOT NULL REFERENCES statements(id) ON DELETE RESTRICT,
                allocated_amount_cents INTEGER NOT NULL CHECK(allocated_amount_cents > 0),
                created_at TEXT NOT NULL,
                UNIQUE(payment_id, statement_id)
            );
            CREATE TABLE IF NOT EXISTS cost_history (
                id INTEGER PRIMARY KEY,
                material_id INTEGER NOT NULL REFERENCES materials(id) ON DELETE RESTRICT,
                cost_cents INTEGER NOT NULL CHECK(cost_cents >= 0),
                cost_date TEXT,
                source TEXT NOT NULL DEFAULT 'manual',
                note TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_payment_allocations_statement ON payment_allocations(statement_id);
            CREATE INDEX IF NOT EXISTS idx_inventory_movements_material ON inventory_movements(material_id, created_at DESC, id DESC);
            CREATE INDEX IF NOT EXISTS idx_stock_reservations_material ON stock_reservations(material_id, status);
            CREATE INDEX IF NOT EXISTS idx_cost_history_material ON cost_history(material_id, cost_date DESC, id DESC);
            INSERT INTO payment_allocations(payment_id, statement_id, allocated_amount_cents, created_at)
            SELECT p.id, p.statement_id, p.amount_cents, p.created_at
            FROM payments p
            WHERE NOT EXISTS (SELECT 1 FROM payment_allocations a WHERE a.payment_id = p.id);
            INSERT INTO cost_history(material_id, cost_cents, cost_date, source, note, created_at)
            SELECT m.id, m.cost_cents, m.cost_date, 'migration', 'V3 迁移：保留现有参考成本。', COALESCE(m.updated_at, datetime('now'))
            FROM materials m
            WHERE NOT EXISTS (SELECT 1 FROM cost_history h WHERE h.material_id = m.id);
            INSERT INTO schema_migrations(version, applied_at) VALUES (3, datetime('now'));
            "#,
        ).map_err(to_error)?;
        conn.execute("UPDATE inventory SET opening_grams=on_hand_grams WHERE opening_grams=0 AND NOT EXISTS (SELECT 1 FROM inventory_movements m WHERE m.material_id=inventory.material_id)", []).map_err(to_error)?;
    }
    let v4_applied: i64 = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=4)",
            [],
            |row| row.get(0),
        )
        .map_err(to_error)?;
    if v4_applied == 0 {
        conn.execute_batch(
            r#"
            CREATE INDEX IF NOT EXISTS idx_attachments_object ON attachments(object_type, object_id, created_at DESC, id DESC);
            INSERT INTO schema_migrations(version, applied_at) VALUES (4, datetime('now'));
            "#,
        ).map_err(to_error)?;
    }
    let v5_applied: i64 = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=5)",
            [],
            |row| row.get(0),
        )
        .map_err(to_error)?;
    if v5_applied == 0 {
        conn.execute_batch(
            r#"
            INSERT OR IGNORE INTO settings(key, value, updated_at) VALUES ('close_behavior','minimize_to_tray',datetime('now'));
            INSERT INTO schema_migrations(version, applied_at) VALUES (5, datetime('now'));
            "#,
        ).map_err(to_error)?;
    }
    let v6_applied: i64 = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=6)",
            [],
            |row| row.get(0),
        )
        .map_err(to_error)?;
    if v6_applied == 0 {
        conn.execute_batch(
            r#"
            INSERT OR IGNORE INTO settings(key, value, updated_at) VALUES
                ('backup_schedule_enabled','1',datetime('now')),
                ('backup_schedule_days','1',datetime('now'));
            INSERT INTO schema_migrations(version, applied_at) VALUES (6, datetime('now'));
            "#,
        )
        .map_err(to_error)?;
    }
    let v7_applied: i64 = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=7)",
            [],
            |row| row.get(0),
        )
        .map_err(to_error)?;
    if v7_applied == 0 {
        ensure_column(
            conn,
            "quotations",
            "seller_name",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(
            conn,
            "quotations",
            "recipient_name",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(
            conn,
            "quotations",
            "sender_name",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(
            conn,
            "quotations",
            "recipient_contact",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(
            conn,
            "quotations",
            "recipient_fax",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(conn, "quotations", "cc", "TEXT NOT NULL DEFAULT ''")?;
        ensure_column(
            conn,
            "quotations",
            "page_count",
            "INTEGER NOT NULL DEFAULT 1",
        )?;
        ensure_column(
            conn,
            "quotations",
            "request_review",
            "INTEGER NOT NULL DEFAULT 1",
        )?;
        ensure_column(
            conn,
            "quotations",
            "request_comment",
            "INTEGER NOT NULL DEFAULT 1",
        )?;
        ensure_column(conn, "quotations", "quote_date", "TEXT")?;
        ensure_column(conn, "quotations", "subject", "TEXT NOT NULL DEFAULT ''")?;
        ensure_column(conn, "quotations", "price_note", "TEXT NOT NULL DEFAULT ''")?;
        ensure_column(
            conn,
            "quotations",
            "adjustment_note",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(
            conn,
            "quotations",
            "footer_address",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(
            conn,
            "quotations",
            "footer_phone",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(conn, "quotations", "footer_fax", "TEXT NOT NULL DEFAULT ''")?;
        ensure_column(
            conn,
            "quotations",
            "footer_email",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(
            conn,
            "quotation_items",
            "material_category",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(
            conn,
            "quotation_items",
            "material_grade",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(
            conn,
            "quotation_items",
            "manufacturer",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(conn, "quotation_items", "note", "TEXT NOT NULL DEFAULT ''")?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS contracts (
                id INTEGER PRIMARY KEY,
                no TEXT NOT NULL UNIQUE,
                seller_name TEXT NOT NULL DEFAULT '',
                buyer_name TEXT NOT NULL DEFAULT '',
                customer_id INTEGER REFERENCES customers(id) ON DELETE RESTRICT,
                execution_place TEXT NOT NULL DEFAULT '',
                contract_date TEXT,
                settlement_method TEXT NOT NULL DEFAULT '',
                packaging TEXT NOT NULL DEFAULT '',
                terms TEXT NOT NULL DEFAULT '',
                seller_address TEXT NOT NULL DEFAULT '',
                seller_legal_representative TEXT NOT NULL DEFAULT '',
                seller_agent TEXT NOT NULL DEFAULT '',
                seller_phone TEXT NOT NULL DEFAULT '',
                seller_fax TEXT NOT NULL DEFAULT '',
                seller_bank TEXT NOT NULL DEFAULT '',
                seller_account TEXT NOT NULL DEFAULT '',
                buyer_address TEXT NOT NULL DEFAULT '',
                buyer_legal_representative TEXT NOT NULL DEFAULT '',
                buyer_agent TEXT NOT NULL DEFAULT '',
                buyer_phone TEXT NOT NULL DEFAULT '',
                buyer_fax TEXT NOT NULL DEFAULT '',
                buyer_bank TEXT NOT NULL DEFAULT '',
                buyer_account TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'draft',
                archived_at TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS contract_items (
                id INTEGER PRIMARY KEY,
                contract_id INTEGER NOT NULL REFERENCES contracts(id) ON DELETE RESTRICT,
                material_id INTEGER REFERENCES materials(id) ON DELETE RESTRICT,
                material_name TEXT NOT NULL DEFAULT '',
                model TEXT NOT NULL DEFAULT '',
                manufacturer TEXT NOT NULL DEFAULT '',
                qty_grams INTEGER NOT NULL DEFAULT 0,
                unit_price_cents INTEGER NOT NULL DEFAULT 0,
                amount_cents INTEGER NOT NULL DEFAULT 0,
                note TEXT NOT NULL DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_contracts_updated ON contracts(updated_at DESC, id DESC);
            CREATE INDEX IF NOT EXISTS idx_contract_items_contract ON contract_items(contract_id, id);
            "#,
        )
        .map_err(to_error)?;
        for (column, definition) in [
            ("seller_address", "TEXT NOT NULL DEFAULT ''"),
            ("seller_legal_representative", "TEXT NOT NULL DEFAULT ''"),
            ("seller_agent", "TEXT NOT NULL DEFAULT ''"),
            ("seller_phone", "TEXT NOT NULL DEFAULT ''"),
            ("seller_fax", "TEXT NOT NULL DEFAULT ''"),
            ("seller_bank", "TEXT NOT NULL DEFAULT ''"),
            ("seller_account", "TEXT NOT NULL DEFAULT ''"),
            ("buyer_address", "TEXT NOT NULL DEFAULT ''"),
            ("buyer_legal_representative", "TEXT NOT NULL DEFAULT ''"),
            ("buyer_agent", "TEXT NOT NULL DEFAULT ''"),
            ("buyer_phone", "TEXT NOT NULL DEFAULT ''"),
            ("buyer_fax", "TEXT NOT NULL DEFAULT ''"),
            ("buyer_bank", "TEXT NOT NULL DEFAULT ''"),
            ("buyer_account", "TEXT NOT NULL DEFAULT ''"),
        ] {
            ensure_column(conn, "contracts", column, definition)?;
        }
        conn.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (7, datetime('now'))",
            [],
        )
        .map_err(to_error)?;
    }
    let v8_applied: i64 = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=8)",
            [],
            |row| row.get(0),
        )
        .map_err(to_error)?;
    if v8_applied == 0 {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS restore_drills (
                id INTEGER PRIMARY KEY,
                backup_path TEXT NOT NULL,
                backup_checksum TEXT NOT NULL,
                backup_created_at TEXT,
                started_at TEXT NOT NULL,
                finished_at TEXT,
                duration_ms INTEGER,
                result TEXT NOT NULL,
                error_summary TEXT NOT NULL DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_restore_drills_started ON restore_drills(started_at DESC, id DESC);
            INSERT INTO schema_migrations(version, applied_at) VALUES (8, datetime('now'));
            "#,
        )
        .map_err(to_error)?;
    }
    let v9_applied: i64 = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=9)",
            [],
            |row| row.get(0),
        )
        .map_err(to_error)?;
    if v9_applied == 0 {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS sync_events (
                id INTEGER PRIMARY KEY,
                event_id TEXT NOT NULL UNIQUE,
                entity_type TEXT NOT NULL,
                entity_id INTEGER NOT NULL,
                operation TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                synced_at TEXT,
                retry_count INTEGER NOT NULL DEFAULT 0,
                last_error TEXT NOT NULL DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_sync_events_pending ON sync_events(synced_at, id);
            INSERT OR IGNORE INTO settings(key, value, updated_at)
            VALUES ('cloud_sync_enabled','0',datetime('now'));
            INSERT INTO schema_migrations(version, applied_at) VALUES (9, datetime('now'));
            "#,
        )
        .map_err(to_error)?;
    }
    Ok(())
}

fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(to_error)?;
    let mut rows = stmt.query([]).map_err(to_error)?;
    while let Some(row) = rows.next().map_err(to_error)? {
        let name: String = row.get(1).map_err(to_error)?;
        if name == column {
            return Ok(());
        }
    }
    conn.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
        [],
    )
    .map_err(to_error)?;
    Ok(())
}

fn integrity_check(conn: &Connection) -> Result<(), String> {
    let result: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(to_error)?;
    if result != "ok" {
        return Err(format!("数据库完整性检查失败：{result}"));
    }
    Ok(())
}

fn str_value(data: &Value, key: &str) -> String {
    data.get(key)
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn optional_str(data: &Value, key: &str) -> Option<String> {
    let value = str_value(data, key);
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn int_value(data: &Value, key: &str) -> i64 {
    match data.get(key) {
        Some(Value::Number(value)) => value
            .as_i64()
            .or_else(|| value.as_f64().map(|v| v.round() as i64))
            .unwrap_or(0),
        Some(Value::String(value)) => value.parse::<f64>().map(|v| v.round() as i64).unwrap_or(0),
        _ => 0,
    }
}

fn bool_int_value(data: &Value, key: &str, default: bool) -> i64 {
    match data.get(key) {
        Some(Value::Bool(value)) => i64::from(*value),
        Some(Value::Number(value)) => i64::from(value.as_i64().unwrap_or(0) != 0),
        Some(Value::String(value)) => i64::from(matches!(
            value.trim().to_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )),
        _ => i64::from(default),
    }
}

fn required_string(data: &Value, key: &str, label: &str) -> Result<String, String> {
    let value = str_value(data, key);
    if value.is_empty() {
        Err(format!("{label}不能为空"))
    } else {
        Ok(value)
    }
}

fn line_amount_cents(qty_grams: i64, unit_price_cents: i64) -> Result<i64, String> {
    if qty_grams <= 0 || unit_price_cents <= 0 {
        return Err("明细数量和单价必须大于 0".into());
    }
    Ok(((qty_grams as i128 * unit_price_cents as i128 + 500) / 1000) as i64)
}

fn item_values(data: &Value) -> Vec<Value> {
    data.get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn entity_exists(tx: &Transaction<'_>, table: &str, id: i64) -> Result<bool, String> {
    if id <= 0 {
        return Ok(false);
    }
    tx.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id=?1)"),
        [id],
        |row| row.get::<_, i64>(0),
    )
    .map(|value| value == 1)
    .map_err(to_error)
}

fn order_total_cents(tx: &Transaction<'_>, order_id: i64) -> Result<i64, String> {
    let item_total: i64 = tx
        .query_row(
            "SELECT COALESCE(SUM(amount_cents),0) FROM order_items WHERE order_id=?1",
            [order_id],
            |row| row.get(0),
        )
        .map_err(to_error)?;
    if item_total > 0 {
        return Ok(item_total);
    }
    tx.query_row(
        "SELECT CAST(ROUND(qty_grams*price_cents/1000.0) AS INTEGER) FROM orders WHERE id=?1",
        [order_id],
        |row| row.get(0),
    )
    .map_err(to_error)
}

fn quotation_total_cents(tx: &Transaction<'_>, quotation_id: i64) -> Result<i64, String> {
    let item_total: i64 = tx
        .query_row(
            "SELECT COALESCE(SUM(amount_cents),0) FROM quotation_items WHERE quotation_id=?1",
            [quotation_id],
            |row| row.get(0),
        )
        .map_err(to_error)?;
    if item_total > 0 {
        return Ok(item_total);
    }
    tx.query_row(
        "SELECT CAST(ROUND(moq_grams*price_cents/1000.0) AS INTEGER) FROM quotations WHERE id=?1",
        [quotation_id],
        |row| row.get(0),
    )
    .map_err(to_error)
}

fn sync_quotation_header(tx: &Transaction<'_>, quotation_id: i64) -> Result<(), String> {
    let first: Option<(i64, i64, i64)> = tx.query_row("SELECT material_id,qty_grams,unit_price_cents FROM quotation_items WHERE quotation_id=?1 ORDER BY id LIMIT 1", [quotation_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional().map_err(to_error)?;
    if let Some((material_id, _, unit_price)) = first {
        let qty: i64 = tx
            .query_row(
                "SELECT COALESCE(SUM(qty_grams),0) FROM quotation_items WHERE quotation_id=?1",
                [quotation_id],
                |row| row.get(0),
            )
            .map_err(to_error)?;
        tx.execute("UPDATE quotations SET material_id=?1,price_cents=?2,moq_grams=?3,updated_at=?4 WHERE id=?5", params![material_id, unit_price, qty, now_iso(), quotation_id]).map_err(to_error)?;
    }
    Ok(())
}

fn sync_order_header(tx: &Transaction<'_>, order_id: i64) -> Result<(), String> {
    let first: Option<(i64, i64)> = tx.query_row("SELECT material_id,unit_price_cents FROM order_items WHERE order_id=?1 ORDER BY id LIMIT 1", [order_id], |row| Ok((row.get(0)?, row.get(1)?))).optional().map_err(to_error)?;
    if let Some((material_id, unit_price)) = first {
        let qty: i64 = tx
            .query_row(
                "SELECT COALESCE(SUM(qty_grams),0) FROM order_items WHERE order_id=?1",
                [order_id],
                |row| row.get(0),
            )
            .map_err(to_error)?;
        tx.execute("UPDATE orders SET material_id=?1,price_cents=?2,qty_grams=?3,updated_at=?4 WHERE id=?5", params![material_id, unit_price, qty, now_iso(), order_id]).map_err(to_error)?;
    }
    Ok(())
}

fn ensure_order_item_from_header(tx: &Transaction<'_>, order_id: i64) -> Result<(), String> {
    let count: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM order_items WHERE order_id=?1",
            [order_id],
            |row| row.get(0),
        )
        .map_err(to_error)?;
    if count > 0 {
        return Ok(());
    }
    let (material_id, qty, price): (i64, i64, i64) = tx
        .query_row(
            "SELECT material_id,qty_grams,price_cents FROM orders WHERE id=?1",
            [order_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(to_error)?;
    let amount = line_amount_cents(qty, price)?;
    tx.execute("INSERT INTO order_items(order_id,material_id,batch,qty_grams,unit_price_cents,amount_cents) VALUES (?1,?2,'',?3,?4,?5)", params![order_id, material_id, qty, price, amount]).map_err(to_error)?;
    Ok(())
}

fn ensure_delivery_items_from_order(tx: &Transaction<'_>, delivery_id: i64) -> Result<(), String> {
    let existing: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM delivery_items WHERE delivery_id=?1",
            [delivery_id],
            |row| row.get(0),
        )
        .map_err(to_error)?;
    if existing > 0 {
        return Ok(());
    }
    let order_id: i64 = tx
        .query_row(
            "SELECT order_id FROM deliveries WHERE id=?1",
            [delivery_id],
            |row| row.get(0),
        )
        .map_err(to_error)?;
    ensure_order_item_from_header(tx, order_id)?;
    let lines: Vec<(i64, i64, String)> = {
        let mut stmt = tx.prepare("SELECT oi.id,oi.qty_grams,oi.batch FROM order_items oi WHERE oi.order_id=?1 ORDER BY oi.id").map_err(to_error)?;
        let rows = stmt
            .query_map([order_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(to_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(to_error)?
    };
    for (order_item_id, ordered_qty, batch) in lines {
        let delivered: i64 = tx
            .query_row(
                "SELECT COALESCE(SUM(qty_grams),0) FROM delivery_items WHERE order_item_id=?1",
                [order_item_id],
                |row| row.get(0),
            )
            .map_err(to_error)?;
        let remaining = ordered_qty - delivered;
        if remaining > 0 {
            tx.execute("INSERT INTO delivery_items(delivery_id,order_item_id,batch,qty_grams) VALUES (?1,?2,?3,?4)", params![delivery_id, order_item_id, batch, remaining]).map_err(to_error)?;
        }
    }
    let count: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM delivery_items WHERE delivery_id=?1",
            [delivery_id],
            |row| row.get(0),
        )
        .map_err(to_error)?;
    if count == 0 {
        return Err("订单已全部送货，不能生成空送货单".into());
    }
    Ok(())
}

fn refresh_statements_for_order(tx: &Transaction<'_>, order_id: i64) -> Result<(), String> {
    let statement_ids: Vec<i64> = {
        let mut stmt = tx
            .prepare("SELECT DISTINCT statement_id FROM statement_items WHERE order_id=?1")
            .map_err(to_error)?;
        let rows = stmt
            .query_map([order_id], |row| row.get(0))
            .map_err(to_error)?;
        rows.collect::<Result<Vec<i64>, _>>().map_err(to_error)?
    };
    for statement_id in statement_ids {
        refresh_statement(tx, statement_id)?;
    }
    Ok(())
}

fn movement_sign(movement_type: &str) -> Option<i64> {
    match movement_type {
        "opening" | "purchase_receipt" | "adjustment_in" | "return_in" => Some(1),
        "sale_issue" | "adjustment_out" | "return_out" => Some(-1),
        _ => None,
    }
}

fn refresh_inventory(tx: &Transaction<'_>, material_id: i64) -> Result<(), String> {
    let opening: i64 = tx
        .query_row(
            "SELECT opening_grams FROM inventory WHERE material_id=?1",
            [material_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(to_error)?
        .unwrap_or(0);
    let movement_sum: i64 = tx.query_row("SELECT COALESCE(SUM(CASE WHEN movement_type IN ('opening','purchase_receipt','adjustment_in','return_in') THEN qty_grams ELSE -qty_grams END),0) FROM inventory_movements WHERE material_id=?1", [material_id], |row| row.get(0)).map_err(to_error)?;
    let transit: i64 = tx.query_row("SELECT COALESCE(SUM(qty_grams),0) FROM purchases WHERE material_id=?1 AND status IN ('ordered','in_transit') AND archived_at IS NULL", [material_id], |row| row.get(0)).map_err(to_error)?;
    tx.execute("UPDATE inventory SET on_hand_grams=?1,in_transit_grams=?2,updated_at=?3 WHERE material_id=?4", params![opening + movement_sum, transit, now_iso(), material_id]).map_err(to_error)?;
    Ok(())
}

fn refresh_order_reservation(tx: &Transaction<'_>, order_id: i64) -> Result<(), String> {
    let (material_id, status): (i64, String) = tx
        .query_row(
            "SELECT material_id,status FROM orders WHERE id=?1",
            [order_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(to_error)?;
    tx.execute(
        "UPDATE stock_reservations SET status='released' WHERE order_id=?1 AND status='active'",
        [order_id],
    )
    .map_err(to_error)?;
    if status == "preparing" {
        let lines: Vec<(i64, i64, String)> = {
            let mut stmt = tx.prepare("SELECT material_id,COALESCE(SUM(qty_grams),0),MAX(batch) FROM order_items WHERE order_id=?1 GROUP BY material_id").map_err(to_error)?;
            let rows = stmt
                .query_map([order_id], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    ))
                })
                .map_err(to_error)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(to_error)?
        };
        let lines = if lines.is_empty() {
            vec![(
                material_id,
                tx.query_row(
                    "SELECT qty_grams FROM orders WHERE id=?1",
                    [order_id],
                    |row| row.get(0),
                )
                .map_err(to_error)?,
                String::new(),
            )]
        } else {
            lines
        };
        for (line_material, qty, batch) in lines {
            if qty <= 0 {
                continue;
            }
            tx.execute("INSERT INTO stock_reservations(material_id,order_id,reserved_grams,batch,warehouse,status,created_at) VALUES (?1,?2,?3,?4,'','active',?5)", params![line_material, order_id, qty, batch, now_iso()]).map_err(to_error)?;
        }
    }
    let materials: Vec<i64> = {
        let mut stmt = tx
            .prepare("SELECT DISTINCT material_id FROM stock_reservations WHERE order_id=?1")
            .map_err(to_error)?;
        let rows = stmt
            .query_map([order_id], |row| row.get(0))
            .map_err(to_error)?;
        rows.collect::<Result<Vec<i64>, _>>().map_err(to_error)?
    };
    for material in materials {
        refresh_inventory(tx, material)?;
    }
    refresh_inventory(tx, material_id)?;
    Ok(())
}

fn record_movement(
    tx: &Transaction<'_>,
    material_id: i64,
    movement_type: &str,
    qty_grams: i64,
    batch: &str,
    warehouse: &str,
    unit_cost_cents: i64,
    source_id: Option<i64>,
) -> Result<i64, String> {
    if !entity_exists(tx, "materials", material_id)? {
        return Err("牌号不存在".into());
    }
    if movement_sign(movement_type).is_none() {
        return Err(format!("库存流水类型无效：{movement_type}"));
    }
    if qty_grams <= 0 {
        return Err("库存流水数量必须大于 0".into());
    }
    if let Some(source_id) = source_id {
        let existing: Option<i64> = tx.query_row("SELECT id FROM inventory_movements WHERE material_id=?1 AND movement_type=?2 AND source_id=?3 LIMIT 1", params![material_id, movement_type, source_id], |row| row.get(0)).optional().map_err(to_error)?;
        if let Some(id) = existing {
            return Ok(id);
        }
    }
    tx.execute("INSERT INTO inventory_movements(material_id,movement_type,qty_grams,batch,warehouse,unit_cost_cents,source_id,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)", params![material_id, movement_type, qty_grams, batch, warehouse, unit_cost_cents.max(0), source_id, now_iso()]).map_err(to_error)?;
    let id = tx.last_insert_rowid();
    refresh_inventory(tx, material_id)?;
    Ok(id)
}

fn record_purchase_receipt(tx: &Transaction<'_>, purchase_id: i64) -> Result<(), String> {
    let (material_id, qty): (i64, i64) = tx
        .query_row(
            "SELECT material_id,qty_grams FROM purchases WHERE id=?1",
            [purchase_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(to_error)?;
    let no: String = tx
        .query_row(
            "SELECT no FROM purchases WHERE id=?1",
            [purchase_id],
            |row| row.get(0),
        )
        .map_err(to_error)?;
    record_movement(
        tx,
        material_id,
        "purchase_receipt",
        qty,
        &no,
        "",
        0,
        Some(purchase_id),
    )?;
    Ok(())
}

fn statement_total_cents(tx: &Transaction<'_>, statement_id: i64) -> Result<i64, String> {
    let item_count: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM statement_items WHERE statement_id=?1",
            [statement_id],
            |row| row.get(0),
        )
        .map_err(to_error)?;
    if item_count > 0 {
        return tx.query_row("SELECT COALESCE(SUM(allocated_amount_cents),0) FROM statement_items WHERE statement_id=?1", [statement_id], |row| row.get(0)).map_err(to_error);
    }
    let (customer_id, start, end): (i64, String, String) = tx
        .query_row(
            "SELECT customer_id,period_start,period_end FROM statements WHERE id=?1",
            [statement_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(to_error)?;
    let order_ids: Vec<i64> = {
        let mut stmt = tx.prepare("SELECT id FROM orders WHERE customer_id=?1 AND delivery_date>=?2 AND delivery_date<=?3 AND archived_at IS NULL").map_err(to_error)?;
        let rows = stmt
            .query_map(params![customer_id, start, end], |row| row.get(0))
            .map_err(to_error)?;
        rows.collect::<Result<Vec<i64>, _>>().map_err(to_error)?
    };
    order_ids.into_iter().try_fold(0_i64, |sum, order_id| {
        sum.checked_add(order_total_cents(tx, order_id)?)
            .ok_or_else(|| "对账金额超出范围".to_string())
    })
}

fn configured_reminder_days(tx: &Transaction<'_>) -> i64 {
    tx.query_row(
        "SELECT value FROM settings WHERE key='reminder_days'",
        [],
        |row| row.get::<_, String>(0),
    )
    .ok()
    .and_then(|value| value.parse::<i64>().ok())
    .map(|value| value.clamp(0, 365))
    .unwrap_or(3)
}

fn statement_received_cents(
    tx: &Transaction<'_>,
    statement_id: i64,
    excluded_payment_id: Option<i64>,
) -> Result<i64, String> {
    // A payment can either have allocation rows (the current model) or only
    // its legacy statement_id/amount_cents fields.  Evaluate each payment
    // independently so a mixed database cannot silently drop one side.
    let excluded = excluded_payment_id.unwrap_or(0);
    tx.query_row(
        r#"
        SELECT COALESCE(SUM(
            CASE
                WHEN EXISTS (SELECT 1 FROM payment_allocations a2 WHERE a2.payment_id=p.id)
                    THEN COALESCE((SELECT SUM(a.allocated_amount_cents)
                                   FROM payment_allocations a
                                   WHERE a.payment_id=p.id AND a.statement_id=?1),0)
                WHEN p.statement_id=?1 THEN p.amount_cents
                ELSE 0
            END
        ),0)
        FROM payments p
        WHERE p.id<>?2
          AND (p.statement_id=?1 OR EXISTS (
              SELECT 1 FROM payment_allocations a3
              WHERE a3.payment_id=p.id AND a3.statement_id=?1
          ))
        "#,
        params![statement_id, excluded],
        |row| row.get(0),
    )
    .map_err(to_error)
}

fn statement_received_excluding_payment(
    tx: &Transaction<'_>,
    statement_id: i64,
    payment_id: i64,
) -> Result<i64, String> {
    statement_received_cents(tx, statement_id, Some(payment_id))
}

fn replace_payment_allocations(
    tx: &Transaction<'_>,
    payment_id: i64,
    allocations: &[Value],
) -> Result<Vec<i64>, String> {
    let amount: i64 = tx
        .query_row(
            "SELECT amount_cents FROM payments WHERE id=?1",
            [payment_id],
            |row| row.get(0),
        )
        .map_err(to_error)?;
    if allocations.is_empty() {
        return Err("至少需要一条回款分配".into());
    }
    let old_allocations: HashMap<i64, i64> = {
        let mut stmt = tx.prepare("SELECT statement_id,allocated_amount_cents FROM payment_allocations WHERE payment_id=?1").map_err(to_error)?;
        let rows = stmt
            .query_map([payment_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(to_error)?;
        rows.collect::<Result<Vec<(i64, i64)>, _>>()
            .map_err(to_error)?
            .into_iter()
            .collect()
    };
    let mut parsed = Vec::with_capacity(allocations.len());
    let mut requested_by_statement = HashMap::<i64, i64>::new();
    let mut statement_ids: Vec<i64> = old_allocations.keys().copied().collect();
    let mut seen = HashSet::new();
    let mut total = 0_i64;
    for allocation in allocations {
        let statement_id = int_value(allocation, "statement_id");
        let allocated = if int_value(allocation, "allocated_amount_cents") > 0 {
            int_value(allocation, "allocated_amount_cents")
        } else {
            int_value(allocation, "amount_cents")
        };
        if !entity_exists(tx, "statements", statement_id)? {
            return Err("回款分配的对账单不存在".into());
        }
        if !seen.insert(statement_id) {
            return Err("同一回款不能重复分配到同一张对账单".into());
        }
        if allocated <= 0 {
            return Err("分配金额必须大于 0".into());
        }
        total = total
            .checked_add(allocated)
            .ok_or_else(|| "分配金额超出范围".to_string())?;
        requested_by_statement.insert(statement_id, allocated);
        if statement_ids.iter().all(|id| *id != statement_id) {
            statement_ids.push(statement_id);
        }
        parsed.push((statement_id, allocated));
    }
    if total != amount {
        return Err(format!(
            "回款分配合计必须等于回款金额（当前 {total} 分，回款 {amount} 分）"
        ));
    }
    for (statement_id, requested) in requested_by_statement {
        let statement_total = statement_total_cents(tx, statement_id)?;
        let received_without_current =
            statement_received_excluding_payment(tx, statement_id, payment_id)?;
        let available = statement_total
            .saturating_sub(received_without_current)
            .max(0);
        if requested > available {
            return Err(format!(
                "对账单可收余额不足，不能分配 {requested} 分（当前可收 {available} 分）"
            ));
        }
    }
    tx.execute(
        "DELETE FROM payment_allocations WHERE payment_id=?1",
        [payment_id],
    )
    .map_err(to_error)?;
    for (statement_id, allocated) in parsed {
        tx.execute("INSERT INTO payment_allocations(payment_id,statement_id,allocated_amount_cents,created_at) VALUES (?1,?2,?3,?4)", params![payment_id, statement_id, allocated, now_iso()]).map_err(to_error)?;
    }
    Ok(statement_ids)
}

fn attachment_object_table(object_type: &str) -> Option<&str> {
    match object_type {
        "customers"
        | "follow_ups"
        | "contacts"
        | "projects"
        | "materials"
        | "samples"
        | "sample_tests"
        | "contracts"
        | "contract_items"
        | "quotations"
        | "orders"
        | "deliveries"
        | "statements"
        | "payments"
        | "suppliers"
        | "purchases"
        | "inventory"
        | "quotation_items"
        | "order_items"
        | "delivery_items"
        | "statement_items"
        | "payment_allocations"
        | "inventory_movements"
        | "stock_reservations"
        | "cost_history" => Some(object_type),
        _ => None,
    }
}

fn validate_attachment_object_type(object_type: &str) -> Result<String, String> {
    let normalized = object_type.trim().to_lowercase();
    if attachment_object_table(&normalized).is_none() {
        return Err(format!("不支持为该对象登记附件：{object_type}"));
    }
    Ok(normalized)
}

fn sanitize_attachment_name(file_name: &str) -> Result<String, String> {
    let candidate = file_name.trim().replace('\\', "/");
    let candidate = candidate.rsplit('/').next().unwrap_or_default().trim();
    if candidate.is_empty() || candidate == "." || candidate == ".." {
        return Err("附件文件名不能为空".into());
    }
    let mut cleaned = String::with_capacity(candidate.len());
    for value in candidate.chars() {
        if value.is_control() {
            continue;
        }
        if "<>:\"/\\|?*".contains(value) {
            cleaned.push('_');
        } else {
            cleaned.push(value);
        }
    }
    let mut cleaned = cleaned
        .trim_matches(|value: char| value == ' ' || value == '.')
        .to_string();
    if cleaned.is_empty() {
        return Err("附件文件名无有效字符".into());
    }
    if cleaned.chars().count() > 120 {
        cleaned = cleaned.chars().take(120).collect();
    }
    Ok(cleaned)
}

fn validate_relative_attachment_path(path: &str) -> Result<String, String> {
    let normalized = path.trim().replace('\\', "/");
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.contains(':')
        || !normalized.starts_with("files/")
        || normalized.split('/').any(|part| {
            part == ".." || part == "." || part.is_empty() || part.chars().any(char::is_control)
        })
    {
        return Err("附件路径必须位于 files/ 下且不能包含绝对路径或 ..".into());
    }
    Ok(normalized)
}

fn attachment_path(root: &Path, relative_path: &str) -> Result<PathBuf, String> {
    let normalized = validate_relative_attachment_path(relative_path)?;
    let path = root.join(&normalized);
    let files_root = root.join("files");
    if path.strip_prefix(&files_root).is_err() {
        return Err("附件路径超出便携 files 目录".into());
    }
    Ok(path)
}

fn attachment_path_encrypted(relative_path: &str) -> bool {
    let normalized = relative_path.replace('\\', "/");
    match normalized.split('/').nth(3) {
        Some("encrypted") => true,
        Some("plain") => false,
        _ => normalized.ends_with(".enc"),
    }
}

fn stored_attachment_encrypted(relative_path: &str, stored: &[u8]) -> bool {
    let normalized = relative_path.replace('\\', "/");
    match normalized.split('/').nth(3) {
        Some("encrypted") => true,
        Some("plain") => false,
        _ => normalized.ends_with(".enc") && stored.starts_with(ENCRYPTED_MAGIC),
    }
}

fn attachment_file_name(relative_path: &str, encrypted: bool) -> String {
    let normalized = relative_path.replace('\\', "/");
    let mut file_name = Path::new(&normalized)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("attachment")
        .to_string();
    if encrypted && file_name.ends_with(".enc") {
        file_name.truncate(file_name.len().saturating_sub(4));
    }
    let parts: Vec<&str> = file_name.splitn(4, '-').collect();
    if parts.len() == 4
        && parts[0].len() == 8
        && parts[0].chars().all(|value| value.is_ascii_digit())
        && parts[2].len() == 12
        && parts[2].chars().all(|value| value.is_ascii_hexdigit())
    {
        parts[3].to_string()
    } else {
        file_name
    }
}

fn attachment_object_exists(
    conn: &Connection,
    object_type: &str,
    object_id: i64,
) -> Result<bool, String> {
    let table = attachment_object_table(object_type)
        .ok_or_else(|| format!("不支持为该对象登记附件：{object_type}"))?;
    if object_id <= 0 {
        return Ok(false);
    }
    conn.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id=?1)"),
        [object_id],
        |row| row.get::<_, i64>(0),
    )
    .map(|value| value == 1)
    .map_err(to_error)
}

fn write_attachment_payload(
    root: &Path,
    relative_path: &str,
    payload: &[u8],
) -> Result<PathBuf, String> {
    let path = attachment_path(root, relative_path)?;
    let parent = path.parent().ok_or_else(|| "附件目录无效".to_string())?;
    fs::create_dir_all(parent).map_err(|error| file_operation_error(parent, error))?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("attachment");
    let temp = parent.join(format!(
        ".{file_name}.{}.tmp",
        hex::encode(random_bytes::<8>())
    ));
    let result = (|| {
        let mut file =
            fs::File::create(&temp).map_err(|error| file_operation_error(&temp, error))?;
        file.write_all(payload)
            .map_err(|error| file_operation_error(&temp, error))?;
        file.sync_all()
            .map_err(|error| file_operation_error(&temp, error))?;
        drop(file);
        fs::rename(&temp, &path).map_err(|error| file_operation_error(&path, error))?;
        Ok::<(), String>(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    Ok(path)
}

fn attachment_content_matches(
    root: &Path,
    master: Option<&[u8; 32]>,
    relative_path: &str,
    expected_hash: &str,
) -> bool {
    let path = match attachment_path(root, relative_path) {
        Ok(path) => path,
        Err(_) => return false,
    };
    let stored = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return false,
    };
    let plain = if stored_attachment_encrypted(relative_path, &stored) {
        match master.and_then(|key| decrypt_bytes(key, &stored).ok()) {
            Some(bytes) => bytes,
            None => return false,
        }
    } else {
        stored
    };
    hex::encode(Sha256::digest(&plain)) == expected_hash
}

fn attachment_info(
    root: &Path,
    master: Option<&[u8; 32]>,
    id: i64,
    object_type: &str,
    object_id: i64,
    relative_path: &str,
    hash: &str,
    created_at: &str,
) -> Value {
    let normalized = relative_path.replace('\\', "/");
    let path = attachment_path(root, &normalized).ok();
    let mut encrypted = attachment_path_encrypted(&normalized);
    let (exists, size_bytes, integrity) = match path.as_ref().map(fs::read) {
        None => (false, 0_i64, "invalid_path"),
        Some(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            (false, 0_i64, "missing")
        }
        Some(Err(_)) => (true, 0_i64, "unreadable"),
        Some(Ok(stored)) => {
            let size = stored.len() as i64;
            let stored_encrypted = stored_attachment_encrypted(&normalized, &stored);
            encrypted = stored_encrypted;
            let plain = if stored_encrypted {
                match master {
                    Some(key) => decrypt_bytes(key, &stored),
                    None => Err("locked".to_string()),
                }
            } else {
                Ok(stored)
            };
            let status = match plain {
                Ok(bytes) if hex::encode(Sha256::digest(&bytes)) == hash => "verified",
                Ok(_) => "checksum_mismatch",
                Err(error) if error == "locked" => "locked",
                Err(_) => "unreadable",
            };
            (true, size, status)
        }
    };
    let file_name = attachment_file_name(&normalized, encrypted);
    json!({"id":id,"object_type":object_type,"object_id":object_id,"relative_path":normalized,"hash":hash,"created_at":created_at,"exists":exists,"size_bytes":size_bytes,"integrity":integrity,"encrypted":encrypted,"file_name":file_name})
}

fn list_attachments_conn(
    conn: &Connection,
    root: &Path,
    master: Option<&[u8; 32]>,
    object_type: Option<&str>,
    object_id: Option<i64>,
) -> Result<Vec<Value>, String> {
    let mut stmt = conn.prepare("SELECT id,object_type,object_id,relative_path,hash,created_at FROM attachments ORDER BY created_at DESC,id DESC").map_err(to_error)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(to_error)?;
    let mut output = Vec::new();
    for row in rows {
        let (id, row_type, row_object_id, relative_path, hash, created_at) =
            row.map_err(to_error)?;
        if object_type.is_some_and(|value| value != row_type)
            || object_id.is_some_and(|value| value != row_object_id)
        {
            continue;
        }
        output.push(attachment_info(
            root,
            master,
            id,
            &row_type,
            row_object_id,
            &relative_path,
            &hash,
            &created_at,
        ));
    }
    Ok(output)
}

fn save_attachment_to_connection(
    conn: &mut Connection,
    root: &Path,
    master: Option<&[u8; 32]>,
    object_type: &str,
    object_id: i64,
    file_name: &str,
    bytes: &[u8],
) -> Result<Value, String> {
    let object_type = validate_attachment_object_type(object_type)?;
    if !attachment_object_exists(conn, &object_type, object_id)? {
        return Err("附件关联对象不存在".into());
    }
    if bytes.is_empty() {
        return Err("附件不能为空".into());
    }
    if bytes.len() > MAX_ATTACHMENT_BYTES {
        return Err(format!(
            "附件大小不能超过 {} MB",
            MAX_ATTACHMENT_BYTES / (1024 * 1024)
        ));
    }
    let safe_name = sanitize_attachment_name(file_name)?;
    let hash = hex::encode(Sha256::digest(bytes));
    let existing = conn.query_row("SELECT id,relative_path,created_at FROM attachments WHERE object_type=?1 AND object_id=?2 AND hash=?3 ORDER BY id DESC LIMIT 1", params![object_type, object_id, hash], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))).optional().map_err(to_error)?;
    let existing_verified = existing.as_ref().is_some_and(|(_, existing_path, _)| {
        attachment_content_matches(root, master, existing_path, &hash)
    });
    if let Some((existing_id, existing_path, existing_created)) =
        existing.as_ref().filter(|_| existing_verified)
    {
        let mut result = attachment_info(
            root,
            master,
            *existing_id,
            &object_type,
            object_id,
            existing_path,
            &hash,
            existing_created,
        );
        if let Value::Object(ref mut fields) = result {
            fields.insert("deduplicated".into(), json!(true));
        }
        return Ok(result);
    }
    let repair_existing = existing.filter(|_| !existing_verified);
    if let Some((existing_id, _, _)) = repair_existing.as_ref() {
        if *existing_id <= 0 {
            return Err("附件记录 ID 无效".into());
        }
    }
    let encrypted = master.is_some();
    let payload = if let Some(key) = master {
        encrypt_bytes(key, bytes)?
    } else {
        bytes.to_vec()
    };
    let stamp = Local::now().format("%Y%m%d-%H%M%S%.3f").to_string();
    let storage_mode = if encrypted { "encrypted" } else { "plain" };
    let relative_path = format!(
        "files/{object_type}/{object_id}/{storage_mode}/{stamp}-{}-{}{}",
        &hash[..12],
        safe_name,
        if encrypted { ".enc" } else { "" }
    );
    write_attachment_payload(root, &relative_path, &payload)?;
    let created_at = now_iso();
    let (id, repaired, old_path, result_created_at) = if let Some((
        existing_id,
        existing_path,
        existing_created,
    )) = repair_existing
    {
        let updated = (|| {
            let tx = conn.transaction().map_err(to_error)?;
            tx.execute(
                "UPDATE attachments SET relative_path=?1 WHERE id=?2",
                params![relative_path, existing_id],
            )
            .map_err(to_error)?;
            audit(
                &tx,
                "attachments",
                existing_id,
                "attachment_repair",
                None,
                Some(json!({"relative_path":relative_path,"hash":hash,"encrypted":encrypted})),
                None,
            )?;
            tx.commit().map_err(to_error)
        })();
        if let Err(error) = updated {
            if let Ok(path) = attachment_path(root, &relative_path) {
                let _ = fs::remove_file(path);
            }
            return Err(error);
        }
        (existing_id, true, Some(existing_path), existing_created)
    } else {
        let inserted = (|| {
            let tx = conn.transaction().map_err(to_error)?;
            tx.execute("INSERT INTO attachments(object_type,object_id,relative_path,hash,created_at) VALUES (?1,?2,?3,?4,?5)", params![object_type, object_id, relative_path, hash, created_at])
                .map_err(to_error)?;
            let id = tx.last_insert_rowid();
            audit(
                &tx,
                &object_type,
                object_id,
                "attachment_add",
                None,
                Some(
                    json!({"attachment_id":id,"relative_path":relative_path,"hash":hash,"encrypted":encrypted}),
                ),
                None,
            )?;
            tx.commit().map_err(to_error)?;
            Ok::<i64, String>(id)
        })();
        let id = match inserted {
            Ok(id) => id,
            Err(error) => {
                if let Ok(path) = attachment_path(root, &relative_path) {
                    let _ = fs::remove_file(path);
                }
                return Err(error);
            }
        };
        (id, false, None, created_at.clone())
    };
    if let Some(old_path) = old_path {
        if old_path != relative_path {
            if let Ok(path) = attachment_path(root, &old_path) {
                let _ = fs::remove_file(path);
            }
        }
    }
    let mut result = attachment_info(
        root,
        master,
        id,
        &object_type,
        object_id,
        &relative_path,
        &hash,
        &result_created_at,
    );
    if let Value::Object(ref mut fields) = result {
        fields.insert("file_name".into(), json!(safe_name));
        fields.insert("deduplicated".into(), json!(false));
        fields.insert("repaired".into(), json!(repaired));
    }
    Ok(result)
}

fn read_attachment_from_connection(
    conn: &Connection,
    root: &Path,
    master: Option<&[u8; 32]>,
    id: i64,
) -> Result<Value, String> {
    let (object_type, object_id, relative_path, hash, created_at): (String, i64, String, String, String) = conn.query_row("SELECT object_type,object_id,relative_path,hash,created_at FROM attachments WHERE id=?1", [id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))).optional().map_err(to_error)?.ok_or_else(|| "附件记录不存在".to_string())?;
    let normalized = validate_relative_attachment_path(&relative_path)?;
    let path = attachment_path(root, &normalized)?;
    let stored = fs::read(&path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            "附件文件不存在".to_string()
        } else {
            file_operation_error(&path, error)
        }
    })?;
    let encrypted = stored_attachment_encrypted(&normalized, &stored);
    let plain = if encrypted {
        let key = master.ok_or_else(|| "当前保护模式未解锁，不能读取附件".to_string())?;
        decrypt_bytes(key, &stored)?
    } else {
        stored
    };
    let actual_hash = hex::encode(Sha256::digest(&plain));
    if actual_hash != hash {
        return Err("附件校验失败，文件可能已被修改".into());
    }
    let file_name = attachment_file_name(&normalized, encrypted);
    Ok(
        json!({"id":id,"object_type":object_type,"object_id":object_id,"relative_path":normalized,"hash":hash,"created_at":created_at,"file_name":file_name,"bytes":plain}),
    )
}

fn decrypt_attachments_for_disable(
    conn: &mut Connection,
    root: &Path,
    master: &[u8; 32],
) -> Result<(), String> {
    let records: Vec<(i64, String, i64, String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT id,object_type,object_id,relative_path,hash FROM attachments ORDER BY id",
            )
            .map_err(to_error)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(to_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(to_error)?
    };
    let mut staged: Vec<(i64, PathBuf, String)> = Vec::new();
    let stage_result = (|| {
        for (id, object_type, object_id, relative_path, hash) in records {
            let normalized = validate_relative_attachment_path(&relative_path)?;
            let old_path = attachment_path(root, &normalized)?;
            if normalized.split('/').nth(3) == Some("plain") {
                continue;
            }
            if !normalized.ends_with(".enc") {
                continue;
            }
            let normalized_hash =
                validate_hash(&hash).map_err(|error| format!("附件 {id} 哈希无效：{error}"))?;
            let stored = fs::read(&old_path).map_err(to_error)?;
            if !stored_attachment_encrypted(&normalized, &stored) {
                continue;
            }
            let plain = decrypt_bytes(master, &stored)?;
            if hex::encode(Sha256::digest(&plain)) != normalized_hash {
                return Err(format!("附件 {id} 校验失败，无法关闭保护模式"));
            }
            let file_name = sanitize_attachment_name(&attachment_file_name(&normalized, true))?;
            let new_relative = if normalized.split('/').nth(3) == Some("encrypted") {
                let stored_name = Path::new(&normalized)
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("attachment");
                let plain_name = stored_name.strip_suffix(".enc").unwrap_or(stored_name);
                format!("files/{object_type}/{object_id}/plain/{plain_name}")
            } else {
                let stamp = Local::now().format("%Y%m%d-%H%M%S%.3f").to_string();
                format!(
                    "files/{object_type}/{object_id}/plain/{stamp}-{}-{}",
                    &normalized_hash[..12],
                    file_name
                )
            };
            write_attachment_payload(root, &new_relative, &plain)?;
            staged.push((id, old_path, new_relative));
        }
        Ok::<(), String>(())
    })();
    if let Err(error) = stage_result {
        for (_, _, relative_path) in &staged {
            if let Ok(path) = attachment_path(root, relative_path) {
                let _ = fs::remove_file(path);
            }
        }
        return Err(error);
    }
    if staged.is_empty() {
        return Ok(());
    }
    let updated = (|| {
        let tx = conn.transaction().map_err(to_error)?;
        for (id, _, relative_path) in &staged {
            tx.execute(
                "UPDATE attachments SET relative_path=?1 WHERE id=?2",
                params![relative_path, id],
            )
            .map_err(to_error)?;
            audit(
                &tx,
                "attachments",
                *id,
                "attachment_decrypt",
                None,
                Some(json!({"relative_path":relative_path,"encrypted":false})),
                None,
            )?;
        }
        tx.commit().map_err(to_error)
    })();
    if let Err(error) = updated {
        for (_, _, relative_path) in &staged {
            if let Ok(path) = attachment_path(root, relative_path) {
                let _ = fs::remove_file(path);
            }
        }
        return Err(error);
    }
    for (_, old_path, _) in staged {
        let _ = fs::remove_file(old_path);
    }
    Ok(())
}

fn encrypt_attachments_for_enable(
    conn: &mut Connection,
    root: &Path,
    master: &[u8; 32],
) -> Result<(), String> {
    let records: Vec<(i64, String, i64, String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT id,object_type,object_id,relative_path,hash FROM attachments ORDER BY id",
            )
            .map_err(to_error)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(to_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(to_error)?
    };
    let mut staged: Vec<(i64, PathBuf, String)> = Vec::new();
    let stage_result = (|| {
        for (id, object_type, object_id, relative_path, hash) in records {
            let normalized = validate_relative_attachment_path(&relative_path)?;
            let old_path = attachment_path(root, &normalized)?;
            let normalized_hash =
                validate_hash(&hash).map_err(|error| format!("附件 {id} 哈希无效：{error}"))?;
            let stored = fs::read(&old_path).map_err(to_error)?;
            if stored_attachment_encrypted(&normalized, &stored) {
                let plain = decrypt_bytes(master, &stored)
                    .map_err(|error| format!("附件 {id} 无法校验：{error}"))?;
                if hex::encode(Sha256::digest(&plain)) != normalized_hash {
                    return Err(format!("附件 {id} 校验失败，无法启用保护模式"));
                }
                continue;
            }
            if hex::encode(Sha256::digest(&stored)) != normalized_hash {
                return Err(format!("附件 {id} 校验失败，无法启用保护模式"));
            }
            let file_name = sanitize_attachment_name(&attachment_file_name(&normalized, false))?;
            let stamp = Local::now().format("%Y%m%d-%H%M%S%.3f").to_string();
            let new_relative = format!(
                "files/{object_type}/{object_id}/encrypted/{stamp}-{}-{}.enc",
                &normalized_hash[..12],
                file_name
            );
            let payload = encrypt_bytes(master, &stored)?;
            write_attachment_payload(root, &new_relative, &payload)?;
            staged.push((id, old_path, new_relative));
        }
        Ok::<(), String>(())
    })();
    if let Err(error) = stage_result {
        for (_, _, relative_path) in &staged {
            if let Ok(path) = attachment_path(root, relative_path) {
                let _ = fs::remove_file(path);
            }
        }
        return Err(error);
    }
    if staged.is_empty() {
        return Ok(());
    }
    let updated = (|| {
        let tx = conn.transaction().map_err(to_error)?;
        for (id, _, relative_path) in &staged {
            tx.execute(
                "UPDATE attachments SET relative_path=?1 WHERE id=?2",
                params![relative_path, id],
            )
            .map_err(to_error)?;
            audit(
                &tx,
                "attachments",
                *id,
                "attachment_encrypt",
                None,
                Some(json!({"relative_path":relative_path,"encrypted":true})),
                None,
            )?;
        }
        tx.commit().map_err(to_error)
    })();
    if let Err(error) = updated {
        for (_, _, relative_path) in &staged {
            if let Ok(path) = attachment_path(root, relative_path) {
                let _ = fs::remove_file(path);
            }
        }
        return Err(error);
    }
    for (_, old_path, _) in staged {
        let _ = fs::remove_file(old_path);
    }
    Ok(())
}

fn validate_hash(hash: &str) -> Result<String, String> {
    let normalized = hash.trim().to_lowercase();
    if normalized.len() != 64 || !normalized.chars().all(|value| value.is_ascii_hexdigit()) {
        return Err("附件哈希必须是 64 位 SHA-256 十六进制值".into());
    }
    Ok(normalized)
}

fn audit(
    tx: &Transaction<'_>,
    entity: &str,
    id: i64,
    event: &str,
    before: Option<Value>,
    after: Option<Value>,
    reason: Option<&str>,
) -> Result<(), String> {
    let created_at = now_iso();
    let payload = json!({
        "before": before.as_ref(),
        "after": after.as_ref(),
        "reason": reason,
    })
    .to_string();
    tx.execute("INSERT INTO audit_logs(object_type, object_id, event_type, before_json, after_json, reason, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)", params![entity, id, event, before.map(|v| v.to_string()), after.map(|v| v.to_string()), reason, created_at]).map_err(to_error)?;
    tx.execute(
        "INSERT INTO sync_events(event_id,entity_type,entity_id,operation,payload_json,created_at)\
         SELECT lower(hex(randomblob(16))),?1,?2,?3,?4,?5\
         WHERE ?1 <> 'settings' AND EXISTS (SELECT 1 FROM settings WHERE key='cloud_sync_enabled' AND value='1')",
        params![entity, id, event, payload, created_at],
    )
    .map_err(to_error)?;
    Ok(())
}

fn list_entities_conn(conn: &Connection, entity: &str) -> Result<Vec<Value>, String> {
    let mut output = Vec::new();
    match entity {
        "customers" => {
            let mut stmt = conn.prepare("SELECT id,name,main_host,direction,material_system,stage,next_follow_date,notes FROM customers WHERE archived_at IS NULL ORDER BY updated_at DESC, id DESC").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "name": r.get::<_,String>(1)?, "main_host": r.get::<_,String>(2)?, "direction": r.get::<_,String>(3)?, "material_system": r.get::<_,String>(4)?, "stage": r.get::<_,String>(5)?, "next_follow_date": r.get::<_,Option<String>>(6)?, "notes": r.get::<_,String>(7)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        "contacts" => {
            let mut stmt = conn.prepare("SELECT c.id,c.customer_id,cu.name,c.name,c.role,c.phone,c.email,c.preferred_channel FROM contacts c JOIN customers cu ON cu.id=c.customer_id WHERE c.archived_at IS NULL ORDER BY c.updated_at DESC").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "customer_id": r.get::<_,i64>(1)?, "customer_name": r.get::<_,String>(2)?, "name": r.get::<_,String>(3)?, "role": r.get::<_,String>(4)?, "phone": r.get::<_,String>(5)?, "email": r.get::<_,String>(6)?, "preferred_channel": r.get::<_,String>(7)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        "projects" => {
            let mut stmt = conn.prepare("SELECT p.id,p.customer_id,c.name,p.name,p.main_host,p.part_name,p.expected_volume_grams,p.stage,p.next_follow_date FROM projects p JOIN customers c ON c.id=p.customer_id WHERE p.archived_at IS NULL ORDER BY p.updated_at DESC").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "customer_id": r.get::<_,i64>(1)?, "customer_name": r.get::<_,String>(2)?, "name": r.get::<_,String>(3)?, "main_host": r.get::<_,String>(4)?, "part_name": r.get::<_,String>(5)?, "expected_volume_grams": r.get::<_,i64>(6)?, "stage": r.get::<_,String>(7)?, "next_follow_date": r.get::<_,Option<String>>(8)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        "materials" => {
            let mut stmt = conn.prepare("SELECT m.id,m.code,m.base_resin,m.modification,m.mi,m.impact,m.hdt,m.density,m.supplier_id,COALESCE(s.name,''),m.cost_cents,m.cost_date FROM materials m LEFT JOIN suppliers s ON s.id=m.supplier_id WHERE m.archived_at IS NULL ORDER BY m.updated_at DESC").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "code": r.get::<_,String>(1)?, "base_resin": r.get::<_,String>(2)?, "modification": r.get::<_,String>(3)?, "mi": r.get::<_,String>(4)?, "impact": r.get::<_,String>(5)?, "hdt": r.get::<_,String>(6)?, "density": r.get::<_,String>(7)?, "supplier_id": r.get::<_,Option<i64>>(8)?, "supplier_name": r.get::<_,String>(9)?, "cost_cents": r.get::<_,i64>(10)?, "cost_date": r.get::<_,Option<String>>(11)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        "samples" => {
            let mut stmt = conn.prepare("SELECT s.id,s.code,s.customer_id,c.name,s.material_id,m.code,s.batch,s.weight_grams,s.sent_date,s.test_items,s.status,s.fail_reason FROM samples s JOIN customers c ON c.id=s.customer_id JOIN materials m ON m.id=s.material_id WHERE s.archived_at IS NULL ORDER BY s.updated_at DESC").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "code": r.get::<_,String>(1)?, "customer_id": r.get::<_,i64>(2)?, "customer_name": r.get::<_,String>(3)?, "material_id": r.get::<_,i64>(4)?, "material_code": r.get::<_,String>(5)?, "batch": r.get::<_,String>(6)?, "weight_grams": r.get::<_,i64>(7)?, "sent_date": r.get::<_,Option<String>>(8)?, "test_items": r.get::<_,String>(9)?, "status": r.get::<_,String>(10)?, "fail_reason": r.get::<_,String>(11)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        "follow_ups" => {
            let mut stmt = conn.prepare("SELECT f.id,f.customer_id,c.name,f.content,f.follow_date,f.next_date FROM follow_ups f JOIN customers c ON c.id=f.customer_id ORDER BY f.follow_date DESC,f.id DESC").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "customer_id": r.get::<_,i64>(1)?, "customer_name": r.get::<_,String>(2)?, "content": r.get::<_,String>(3)?, "follow_date": r.get::<_,Option<String>>(4)?, "next_date": r.get::<_,Option<String>>(5)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        "sample_tests" => {
            let mut stmt = conn.prepare("SELECT t.id,t.sample_id,s.code,c.name,t.test_date,t.test_item,t.result,t.conclusion,t.next_action FROM sample_tests t JOIN samples s ON s.id=t.sample_id JOIN customers c ON c.id=s.customer_id ORDER BY t.test_date DESC,t.id DESC").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "sample_id": r.get::<_,i64>(1)?, "sample_code": r.get::<_,String>(2)?, "customer_name": r.get::<_,String>(3)?, "test_date": r.get::<_,String>(4)?, "test_item": r.get::<_,String>(5)?, "result": r.get::<_,String>(6)?, "conclusion": r.get::<_,String>(7)?, "next_action": r.get::<_,String>(8)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        "contracts" => {
            let mut stmt = conn.prepare("SELECT c.id,c.no,c.seller_name,c.buyer_name,c.customer_id,c.execution_place,c.contract_date,c.settlement_method,c.packaging,c.terms,c.seller_address,c.seller_legal_representative,c.seller_agent,c.seller_phone,c.seller_fax,c.seller_bank,c.seller_account,c.buyer_address,c.buyer_legal_representative,c.buyer_agent,c.buyer_phone,c.buyer_fax,c.buyer_bank,c.buyer_account,c.status,COALESCE((SELECT SUM(i.amount_cents) FROM contract_items i WHERE i.contract_id=c.id),0),COALESCE((SELECT COUNT(*) FROM contract_items i WHERE i.contract_id=c.id),0) FROM contracts c WHERE c.archived_at IS NULL ORDER BY c.updated_at DESC,c.id DESC").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "no": r.get::<_,String>(1)?, "seller_name": r.get::<_,String>(2)?, "buyer_name": r.get::<_,String>(3)?, "customer_id": r.get::<_,Option<i64>>(4)?, "execution_place": r.get::<_,String>(5)?, "contract_date": r.get::<_,Option<String>>(6)?, "settlement_method": r.get::<_,String>(7)?, "packaging": r.get::<_,String>(8)?, "terms": r.get::<_,String>(9)?, "seller_address": r.get::<_,String>(10)?, "seller_legal_representative": r.get::<_,String>(11)?, "seller_agent": r.get::<_,String>(12)?, "seller_phone": r.get::<_,String>(13)?, "seller_fax": r.get::<_,String>(14)?, "seller_bank": r.get::<_,String>(15)?, "seller_account": r.get::<_,String>(16)?, "buyer_address": r.get::<_,String>(17)?, "buyer_legal_representative": r.get::<_,String>(18)?, "buyer_agent": r.get::<_,String>(19)?, "buyer_phone": r.get::<_,String>(20)?, "buyer_fax": r.get::<_,String>(21)?, "buyer_bank": r.get::<_,String>(22)?, "buyer_account": r.get::<_,String>(23)?, "status": r.get::<_,String>(24)?, "item_total_cents": r.get::<_,i64>(25)?, "item_count": r.get::<_,i64>(26)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        "quotations" => {
            let mut stmt = conn.prepare("SELECT q.id,q.no,q.customer_id,c.name,q.material_id,m.code,q.price_cents,q.moq_grams,q.freight,q.valid_until,q.seller_name,q.recipient_name,q.sender_name,q.recipient_contact,q.recipient_fax,q.cc,q.page_count,q.request_review,q.request_comment,q.quote_date,q.subject,q.price_note,q.adjustment_note,q.footer_address,q.footer_phone,q.footer_fax,q.footer_email,q.version,q.status,COALESCE((SELECT SUM(i.amount_cents) FROM quotation_items i WHERE i.quotation_id=q.id),0),COALESCE((SELECT COUNT(*) FROM quotation_items i WHERE i.quotation_id=q.id),0) FROM quotations q JOIN customers c ON c.id=q.customer_id JOIN materials m ON m.id=q.material_id WHERE q.archived_at IS NULL ORDER BY q.updated_at DESC").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "no": r.get::<_,String>(1)?, "customer_id": r.get::<_,i64>(2)?, "customer_name": r.get::<_,String>(3)?, "material_id": r.get::<_,i64>(4)?, "material_code": r.get::<_,String>(5)?, "price_cents": r.get::<_,i64>(6)?, "moq_grams": r.get::<_,i64>(7)?, "freight": r.get::<_,String>(8)?, "valid_until": r.get::<_,Option<String>>(9)?, "seller_name": r.get::<_,String>(10)?, "recipient_name": r.get::<_,String>(11)?, "sender_name": r.get::<_,String>(12)?, "recipient_contact": r.get::<_,String>(13)?, "recipient_fax": r.get::<_,String>(14)?, "cc": r.get::<_,String>(15)?, "page_count": r.get::<_,i64>(16)?, "request_review": r.get::<_,i64>(17)? != 0, "request_comment": r.get::<_,i64>(18)? != 0, "quote_date": r.get::<_,Option<String>>(19)?, "subject": r.get::<_,String>(20)?, "price_note": r.get::<_,String>(21)?, "adjustment_note": r.get::<_,String>(22)?, "footer_address": r.get::<_,String>(23)?, "footer_phone": r.get::<_,String>(24)?, "footer_fax": r.get::<_,String>(25)?, "footer_email": r.get::<_,String>(26)?, "version": r.get::<_,i64>(27)?, "status": r.get::<_,String>(28)?, "item_total_cents": r.get::<_,i64>(29)?, "item_count": r.get::<_,i64>(30)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        "orders" => {
            let mut stmt = conn.prepare("SELECT o.id,o.no,o.customer_id,c.name,o.material_id,m.code,o.qty_grams,o.price_cents,o.delivery_date,o.status,o.sign_status,o.quotation_id,COALESCE(NULLIF((SELECT SUM(i.amount_cents) FROM order_items i WHERE i.order_id=o.id),0),CAST(ROUND(o.qty_grams*o.price_cents/1000.0) AS INTEGER)),COALESCE((SELECT COUNT(*) FROM order_items i WHERE i.order_id=o.id),0) FROM orders o JOIN customers c ON c.id=o.customer_id JOIN materials m ON m.id=o.material_id WHERE o.archived_at IS NULL ORDER BY o.updated_at DESC").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "no": r.get::<_,String>(1)?, "customer_id": r.get::<_,i64>(2)?, "customer_name": r.get::<_,String>(3)?, "material_id": r.get::<_,i64>(4)?, "material_code": r.get::<_,String>(5)?, "qty_grams": r.get::<_,i64>(6)?, "price_cents": r.get::<_,i64>(7)?, "delivery_date": r.get::<_,Option<String>>(8)?, "status": r.get::<_,String>(9)?, "sign_status": r.get::<_,String>(10)?, "quotation_id": r.get::<_,Option<i64>>(11)?, "amount_cents": r.get::<_,i64>(12)?, "item_count": r.get::<_,i64>(13)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        "deliveries" => {
            let mut stmt = conn.prepare("SELECT d.id,d.no,d.order_id,o.no,c.name,d.sent_date,d.sign_date FROM deliveries d JOIN orders o ON o.id=d.order_id JOIN customers c ON c.id=o.customer_id WHERE d.archived_at IS NULL ORDER BY d.updated_at DESC").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "no": r.get::<_,String>(1)?, "order_id": r.get::<_,i64>(2)?, "order_no": r.get::<_,String>(3)?, "customer_name": r.get::<_,String>(4)?, "sent_date": r.get::<_,Option<String>>(5)?, "sign_date": r.get::<_,Option<String>>(6)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        "statements" => {
            let mut stmt = conn.prepare("SELECT s.id,s.no,s.customer_id,c.name,s.period_start,s.period_end,s.total_cents,s.received_cents,s.unpaid_cents,s.account_days,s.aging_days,s.promise_date,s.pay_status FROM statements s JOIN customers c ON c.id=s.customer_id WHERE s.archived_at IS NULL ORDER BY s.updated_at DESC").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "no": r.get::<_,String>(1)?, "customer_id": r.get::<_,i64>(2)?, "customer_name": r.get::<_,String>(3)?, "period_start": r.get::<_,String>(4)?, "period_end": r.get::<_,String>(5)?, "total_cents": r.get::<_,i64>(6)?, "received_cents": r.get::<_,i64>(7)?, "unpaid_cents": r.get::<_,i64>(8)?, "account_days": r.get::<_,i64>(9)?, "aging_days": r.get::<_,i64>(10)?, "promise_date": r.get::<_,Option<String>>(11)?, "pay_status": r.get::<_,String>(12)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        "payments" => {
            let mut stmt = conn.prepare("SELECT p.id,p.statement_id,s.no,p.amount_cents,p.pay_date,p.method,COALESCE((SELECT GROUP_CONCAT(sa.no, '、') FROM payment_allocations pa JOIN statements sa ON sa.id=pa.statement_id WHERE pa.payment_id=p.id),s.no) FROM payments p JOIN statements s ON s.id=p.statement_id ORDER BY p.pay_date DESC,p.id DESC").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "statement_id": r.get::<_,i64>(1)?, "statement_no": r.get::<_,String>(2)?, "amount_cents": r.get::<_,i64>(3)?, "pay_date": r.get::<_,String>(4)?, "method": r.get::<_,String>(5)?, "allocation_statements": r.get::<_,String>(6)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        "suppliers" => {
            let mut stmt = conn.prepare("SELECT id,name,contact,price_ref_cents FROM suppliers WHERE archived_at IS NULL ORDER BY updated_at DESC").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "name": r.get::<_,String>(1)?, "contact": r.get::<_,String>(2)?, "price_ref_cents": r.get::<_,i64>(3)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        "purchases" => {
            let mut stmt = conn.prepare("SELECT p.id,p.no,p.supplier_id,s.name,p.material_id,m.code,p.qty_grams,p.status,p.eta FROM purchases p JOIN suppliers s ON s.id=p.supplier_id JOIN materials m ON m.id=p.material_id WHERE p.archived_at IS NULL ORDER BY p.updated_at DESC").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "no": r.get::<_,String>(1)?, "supplier_id": r.get::<_,i64>(2)?, "supplier_name": r.get::<_,String>(3)?, "material_id": r.get::<_,i64>(4)?, "material_code": r.get::<_,String>(5)?, "qty_grams": r.get::<_,i64>(6)?, "status": r.get::<_,String>(7)?, "eta": r.get::<_,Option<String>>(8)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        "inventory" => {
            let mut stmt = conn.prepare("SELECT i.id,i.material_id,m.code,i.opening_grams,i.on_hand_grams,i.in_transit_grams,i.safety_grams,COALESCE((SELECT SUM(r.reserved_grams) FROM stock_reservations r WHERE r.material_id=i.material_id AND r.status='active'),0),(i.on_hand_grams+i.in_transit_grams-COALESCE((SELECT SUM(r.reserved_grams) FROM stock_reservations r WHERE r.material_id=i.material_id AND r.status='active'),0)) FROM inventory i JOIN materials m ON m.id=i.material_id ORDER BY m.code").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "material_id": r.get::<_,i64>(1)?, "material_code": r.get::<_,String>(2)?, "opening_grams": r.get::<_,i64>(3)?, "on_hand_grams": r.get::<_,i64>(4)?, "in_transit_grams": r.get::<_,i64>(5)?, "safety_grams": r.get::<_,i64>(6)?, "reserved_grams": r.get::<_,i64>(7)?, "available_grams": r.get::<_,i64>(8)?, "low_stock": r.get::<_,i64>(8)? < r.get::<_,i64>(6)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        "contract_items" => {
            let mut stmt = conn.prepare("SELECT i.id,i.contract_id,c.no,i.material_id,COALESCE(i.material_name,''),i.model,i.manufacturer,i.qty_grams,i.unit_price_cents,i.amount_cents,i.note FROM contract_items i JOIN contracts c ON c.id=i.contract_id WHERE c.archived_at IS NULL ORDER BY c.no,i.id").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "contract_id": r.get::<_,i64>(1)?, "contract_no": r.get::<_,String>(2)?, "material_id": r.get::<_,Option<i64>>(3)?, "material_name": r.get::<_,String>(4)?, "model": r.get::<_,String>(5)?, "manufacturer": r.get::<_,String>(6)?, "qty_grams": r.get::<_,i64>(7)?, "unit_price_cents": r.get::<_,i64>(8)?, "amount_cents": r.get::<_,i64>(9)?, "note": r.get::<_,String>(10)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        "quotation_items" => {
            let mut stmt = conn.prepare("SELECT i.id,i.quotation_id,q.no,i.material_id,m.code,i.batch,i.material_category,i.material_grade,i.manufacturer,i.note,i.qty_grams,i.unit_price_cents,i.amount_cents FROM quotation_items i JOIN quotations q ON q.id=i.quotation_id JOIN materials m ON m.id=i.material_id WHERE q.archived_at IS NULL ORDER BY q.no,i.id").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "quotation_id": r.get::<_,i64>(1)?, "quotation_no": r.get::<_,String>(2)?, "material_id": r.get::<_,i64>(3)?, "material_code": r.get::<_,String>(4)?, "batch": r.get::<_,String>(5)?, "material_category": r.get::<_,String>(6)?, "material_grade": r.get::<_,String>(7)?, "manufacturer": r.get::<_,String>(8)?, "note": r.get::<_,String>(9)?, "qty_grams": r.get::<_,i64>(10)?, "unit_price_cents": r.get::<_,i64>(11)?, "amount_cents": r.get::<_,i64>(12)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        "order_items" => {
            let mut stmt = conn.prepare("SELECT i.id,i.order_id,o.no,i.material_id,m.code,i.batch,i.qty_grams,i.unit_price_cents,i.amount_cents FROM order_items i JOIN orders o ON o.id=i.order_id JOIN materials m ON m.id=i.material_id WHERE o.archived_at IS NULL ORDER BY o.no,i.id").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "order_id": r.get::<_,i64>(1)?, "order_no": r.get::<_,String>(2)?, "material_id": r.get::<_,i64>(3)?, "material_code": r.get::<_,String>(4)?, "batch": r.get::<_,String>(5)?, "qty_grams": r.get::<_,i64>(6)?, "unit_price_cents": r.get::<_,i64>(7)?, "amount_cents": r.get::<_,i64>(8)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        "delivery_items" => {
            let mut stmt = conn.prepare("SELECT i.id,i.delivery_id,d.no,d.order_id,o.no,i.order_item_id,i.batch,i.qty_grams FROM delivery_items i JOIN deliveries d ON d.id=i.delivery_id JOIN orders o ON o.id=d.order_id WHERE d.archived_at IS NULL ORDER BY d.no,i.id").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "delivery_id": r.get::<_,i64>(1)?, "delivery_no": r.get::<_,String>(2)?, "order_id": r.get::<_,i64>(3)?, "order_no": r.get::<_,String>(4)?, "order_item_id": r.get::<_,Option<i64>>(5)?, "batch": r.get::<_,String>(6)?, "qty_grams": r.get::<_,i64>(7)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        "statement_items" => {
            let mut stmt = conn.prepare("SELECT i.id,i.statement_id,s.no,i.order_id,o.no,i.allocated_amount_cents FROM statement_items i JOIN statements s ON s.id=i.statement_id JOIN orders o ON o.id=i.order_id WHERE s.archived_at IS NULL ORDER BY s.no,i.id").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "statement_id": r.get::<_,i64>(1)?, "statement_no": r.get::<_,String>(2)?, "order_id": r.get::<_,i64>(3)?, "order_no": r.get::<_,String>(4)?, "allocated_amount_cents": r.get::<_,i64>(5)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        "payment_allocations" => {
            let mut stmt = conn.prepare("SELECT a.id,a.payment_id,p.amount_cents,p.pay_date,a.statement_id,s.no,a.allocated_amount_cents FROM payment_allocations a JOIN payments p ON p.id=a.payment_id JOIN statements s ON s.id=a.statement_id ORDER BY p.pay_date DESC,a.id DESC").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "payment_id": r.get::<_,i64>(1)?, "payment_amount_cents": r.get::<_,i64>(2)?, "pay_date": r.get::<_,String>(3)?, "statement_id": r.get::<_,i64>(4)?, "statement_no": r.get::<_,String>(5)?, "allocated_amount_cents": r.get::<_,i64>(6)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        "inventory_movements" => {
            let mut stmt = conn.prepare("SELECT i.id,i.material_id,m.code,i.movement_type,i.qty_grams,i.batch,i.warehouse,i.unit_cost_cents,i.source_id,i.created_at FROM inventory_movements i JOIN materials m ON m.id=i.material_id ORDER BY i.created_at DESC,i.id DESC").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "material_id": r.get::<_,i64>(1)?, "material_code": r.get::<_,String>(2)?, "movement_type": r.get::<_,String>(3)?, "qty_grams": r.get::<_,i64>(4)?, "batch": r.get::<_,String>(5)?, "warehouse": r.get::<_,String>(6)?, "unit_cost_cents": r.get::<_,i64>(7)?, "source_id": r.get::<_,Option<i64>>(8)?, "created_at": r.get::<_,String>(9)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        "stock_reservations" => {
            let mut stmt = conn.prepare("SELECT r.id,r.material_id,m.code,r.order_id,o.no,r.reserved_grams,r.batch,r.warehouse,r.status,r.created_at FROM stock_reservations r JOIN materials m ON m.id=r.material_id LEFT JOIN orders o ON o.id=r.order_id ORDER BY r.created_at DESC,r.id DESC").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "material_id": r.get::<_,i64>(1)?, "material_code": r.get::<_,String>(2)?, "order_id": r.get::<_,Option<i64>>(3)?, "order_no": r.get::<_,Option<String>>(4)?, "reserved_grams": r.get::<_,i64>(5)?, "batch": r.get::<_,String>(6)?, "warehouse": r.get::<_,String>(7)?, "status": r.get::<_,String>(8)?, "created_at": r.get::<_,String>(9)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        "attachments" => {
            let mut stmt = conn.prepare("SELECT id,object_type,object_id,relative_path,hash,created_at FROM attachments ORDER BY created_at DESC,id DESC").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "object_type": r.get::<_,String>(1)?, "object_id": r.get::<_,i64>(2)?, "relative_path": r.get::<_,String>(3)?, "hash": r.get::<_,String>(4)?, "created_at": r.get::<_,String>(5)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        "cost_history" => {
            let mut stmt = conn.prepare("SELECT h.id,h.material_id,m.code,h.cost_cents,h.cost_date,h.source,h.note,h.created_at FROM cost_history h JOIN materials m ON m.id=h.material_id ORDER BY h.cost_date DESC,h.id DESC").map_err(to_error)?;
            let rows = stmt.query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "material_id": r.get::<_,i64>(1)?, "material_code": r.get::<_,String>(2)?, "cost_cents": r.get::<_,i64>(3)?, "cost_date": r.get::<_,Option<String>>(4)?, "source": r.get::<_,String>(5)?, "note": r.get::<_,String>(6)?, "created_at": r.get::<_,String>(7)?}))).map_err(to_error)?;
            for row in rows {
                output.push(row.map_err(to_error)?);
            }
        }
        _ => return Err(format!("不支持的业务对象：{entity}")),
    }
    Ok(output)
}

fn filter_records(
    rows: Vec<Value>,
    search: Option<&str>,
    status: Option<&str>,
    status_key: Option<&str>,
) -> Vec<Value> {
    let needle = search.unwrap_or_default().trim().to_lowercase();
    let status_value = status.unwrap_or_default().trim();
    rows.into_iter()
        .filter(|row| {
            let text = row
                .as_object()
                .map(|object| {
                    object
                        .values()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                        .to_lowercase()
                })
                .unwrap_or_default();
            let status_ok = status_value.is_empty()
                || status_key
                    .and_then(|key| row.get(key))
                    .and_then(Value::as_str)
                    .map(|value| value == status_value)
                    .unwrap_or(false);
            (needle.is_empty() || text.contains(&needle)) && status_ok
        })
        .collect()
}

fn paginate_records(rows: Vec<Value>, page: i64, page_size: i64) -> Result<Value, String> {
    let page = page.max(1);
    let page_size = page_size.clamp(1, 100);
    let total = rows.len() as i64;
    let start = page.saturating_sub(1).saturating_mul(page_size);
    let page_rows = if start >= total {
        Vec::new()
    } else {
        rows.into_iter()
            .skip(start as usize)
            .take(page_size as usize)
            .collect::<Vec<_>>()
    };
    Ok(json!({
        "rows": page_rows,
        "total": total,
        "page": page,
        "page_size": page_size,
    }))
}

fn current_record(conn: &Connection, entity: &str, id: i64) -> Result<Value, String> {
    list_entities_conn(conn, entity)?
        .into_iter()
        .find(|row| row.get("id").and_then(Value::as_i64) == Some(id))
        .ok_or_else(|| "记录不存在或已归档".to_string())
}

fn list_document_items_conn(
    conn: &Connection,
    document_type: &str,
    document_id: i64,
) -> Result<Vec<Value>, String> {
    let entity = match document_type {
        "contracts" => "contract_items",
        "quotations" => "quotation_items",
        "orders" => "order_items",
        "deliveries" => "delivery_items",
        "statements" => "statement_items",
        "payments" => "payment_allocations",
        _ => return Err(format!("不支持的明细单据类型：{document_type}")),
    };
    let key = match entity {
        "contract_items" => "contract_id",
        "quotation_items" => "quotation_id",
        "order_items" => "order_id",
        "delivery_items" => "delivery_id",
        "statement_items" => "statement_id",
        "payment_allocations" => "payment_id",
        _ => unreachable!(),
    };
    Ok(list_entities_conn(conn, entity)?
        .into_iter()
        .filter(|row| row.get(key).and_then(Value::as_i64) == Some(document_id))
        .collect())
}

fn refresh_statement(tx: &Transaction<'_>, statement_id: i64) -> Result<(), String> {
    let (customer_id, start, end, account_days, promise_date): (i64, String, String, i64, Option<String>) = tx.query_row("SELECT customer_id,period_start,period_end,account_days,promise_date FROM statements WHERE id=?1", [statement_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).map_err(to_error)?;
    let item_count: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM statement_items WHERE statement_id=?1",
            [statement_id],
            |r| r.get(0),
        )
        .map_err(to_error)?;
    let total: i64 = if item_count > 0 {
        tx.query_row("SELECT COALESCE(SUM(allocated_amount_cents),0) FROM statement_items WHERE statement_id=?1", [statement_id], |r| r.get(0)).map_err(to_error)?
    } else {
        tx.query_row("SELECT COALESCE(SUM(COALESCE(NULLIF((SELECT SUM(i.amount_cents) FROM order_items i WHERE i.order_id=o.id),0),CAST(ROUND(o.qty_grams*o.price_cents/1000.0) AS INTEGER))),0) FROM orders o WHERE o.customer_id=?1 AND o.delivery_date>=?2 AND o.delivery_date<=?3 AND o.archived_at IS NULL", params![customer_id, start, end], |r| r.get(0)).map_err(to_error)?
    };
    let received = statement_received_cents(tx, statement_id, None)?;
    let unpaid = (total - received).max(0);
    let aging = promise_date
        .as_deref()
        .and_then(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
        .map(|date| (Local::now().date_naive() - date).num_days().max(0))
        .unwrap_or(0);
    let reminder_days = configured_reminder_days(tx);
    let status = if unpaid <= 0 {
        "settled"
    } else if let Some(date) = promise_date
        .as_deref()
        .and_then(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
    {
        let days = (date - Local::now().date_naive()).num_days();
        if days < 0 {
            "overdue"
        } else if days <= reminder_days {
            "near_due"
        } else {
            "not_due"
        }
    } else if account_days > 0 {
        "not_due"
    } else {
        "not_due"
    };
    tx.execute("UPDATE statements SET total_cents=?1,received_cents=?2,unpaid_cents=?3,aging_days=?4,pay_status=?5,updated_at=?6 WHERE id=?7", params![total, received, unpaid, aging, status, now_iso(), statement_id]).map_err(to_error)?;
    Ok(())
}

fn refresh_customer_next_follow_date(tx: &Transaction<'_>, customer_id: i64) -> Result<(), String> {
    let next_date = tx.query_row(
        "SELECT next_date FROM follow_ups WHERE customer_id=?1 ORDER BY follow_date DESC,id DESC LIMIT 1",
        [customer_id],
        |row| row.get::<_, Option<String>>(0),
    ).optional().map_err(to_error)?.flatten();
    tx.execute(
        "UPDATE customers SET next_follow_date=?1,updated_at=?2 WHERE id=?3",
        params![next_date, now_iso(), customer_id],
    )
    .map_err(to_error)?;
    Ok(())
}

fn save_document_items_tx(
    tx: &Transaction<'_>,
    document_type: &str,
    document_id: i64,
    items: &[Value],
) -> Result<i64, String> {
    if document_id <= 0 {
        return Err("单据 ID 必须为正数".into());
    }
    if items.is_empty() {
        return Err("至少需要一条明细".into());
    }
    match document_type {
        "contracts" => {
            if !entity_exists(tx, "contracts", document_id)? {
                return Err("合同不存在".into());
            }
            tx.execute(
                "DELETE FROM contract_items WHERE contract_id=?1",
                [document_id],
            )
            .map_err(to_error)?;
            for item in items {
                let material_id = int_value(item, "material_id");
                let material_id = if material_id > 0 {
                    if !entity_exists(tx, "materials", material_id)? {
                        return Err("合同明细牌号不存在".into());
                    }
                    Some(material_id)
                } else {
                    None
                };
                let material_name = str_value(item, "material_name");
                let model = required_string(item, "model", "合同明细型号")?;
                let manufacturer = str_value(item, "manufacturer");
                let qty = int_value(item, "qty_grams");
                let unit_price = if int_value(item, "unit_price_cents") > 0 {
                    int_value(item, "unit_price_cents")
                } else {
                    int_value(item, "price_cents")
                };
                if qty <= 0 || unit_price <= 0 {
                    return Err("合同明细数量和含税单价必须大于 0".into());
                }
                let snapshot_name = if material_name.trim().is_empty() {
                    material_id
                        .and_then(|id| {
                            tx.query_row("SELECT code FROM materials WHERE id=?1", [id], |row| {
                                row.get::<_, String>(0)
                            })
                            .optional()
                            .ok()
                            .flatten()
                        })
                        .unwrap_or_default()
                } else {
                    material_name
                };
                let amount = line_amount_cents(qty, unit_price)?;
                tx.execute("INSERT INTO contract_items(contract_id,material_id,material_name,model,manufacturer,qty_grams,unit_price_cents,amount_cents,note) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![document_id, material_id, snapshot_name, model, manufacturer, qty, unit_price, amount, str_value(item, "note")]).map_err(to_error)?;
            }
            return tx
                .query_row(
                    "SELECT COALESCE(SUM(amount_cents),0) FROM contract_items WHERE contract_id=?1",
                    [document_id],
                    |row| row.get(0),
                )
                .map_err(to_error);
        }
        "quotations" => {
            if !entity_exists(tx, "quotations", document_id)? {
                return Err("报价单不存在".into());
            }
            tx.execute(
                "DELETE FROM quotation_items WHERE quotation_id=?1",
                [document_id],
            )
            .map_err(to_error)?;
            for item in items {
                let material_id = int_value(item, "material_id");
                let qty = int_value(item, "qty_grams");
                let unit_price = if int_value(item, "unit_price_cents") > 0 {
                    int_value(item, "unit_price_cents")
                } else {
                    int_value(item, "price_cents")
                };
                if !entity_exists(tx, "materials", material_id)? {
                    return Err("报价明细牌号不存在".into());
                }
                let amount = line_amount_cents(qty, unit_price)?;
                tx.execute("INSERT INTO quotation_items(quotation_id,material_id,batch,material_category,material_grade,manufacturer,note,qty_grams,unit_price_cents,amount_cents) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)", params![document_id, material_id, str_value(item, "batch"), str_value(item, "material_category"), str_value(item, "material_grade"), str_value(item, "manufacturer"), str_value(item, "note"), qty, unit_price, amount]).map_err(to_error)?;
            }
            sync_quotation_header(tx, document_id)?;
            Ok(quotation_total_cents(tx, document_id)?)
        }
        "orders" => {
            if !entity_exists(tx, "orders", document_id)? {
                return Err("订单不存在".into());
            }
            let delivered_lines: i64 = tx.query_row("SELECT COUNT(*) FROM delivery_items i JOIN order_items oi ON oi.id=i.order_item_id WHERE oi.order_id=?1", [document_id], |row| row.get(0)).map_err(to_error)?;
            if delivered_lines > 0 {
                return Err("订单已有送货明细，不能整体替换订单明细".into());
            }
            tx.execute("DELETE FROM order_items WHERE order_id=?1", [document_id])
                .map_err(to_error)?;
            for item in items {
                let material_id = int_value(item, "material_id");
                let qty = int_value(item, "qty_grams");
                let unit_price = if int_value(item, "unit_price_cents") > 0 {
                    int_value(item, "unit_price_cents")
                } else {
                    int_value(item, "price_cents")
                };
                if !entity_exists(tx, "materials", material_id)? {
                    return Err("订单明细牌号不存在".into());
                }
                let amount = line_amount_cents(qty, unit_price)?;
                tx.execute("INSERT INTO order_items(order_id,material_id,batch,qty_grams,unit_price_cents,amount_cents) VALUES (?1,?2,?3,?4,?5,?6)", params![document_id, material_id, str_value(item, "batch"), qty, unit_price, amount]).map_err(to_error)?;
            }
            sync_order_header(tx, document_id)?;
            refresh_order_reservation(tx, document_id)?;
            refresh_statements_for_order(tx, document_id)?;
            Ok(order_total_cents(tx, document_id)?)
        }
        "deliveries" => {
            if !entity_exists(tx, "deliveries", document_id)? {
                return Err("送货单不存在".into());
            }
            let order_id: i64 = tx
                .query_row(
                    "SELECT order_id FROM deliveries WHERE id=?1",
                    [document_id],
                    |row| row.get(0),
                )
                .map_err(to_error)?;
            ensure_order_item_from_header(tx, order_id)?;
            tx.execute("DELETE FROM inventory_movements WHERE movement_type='sale_issue' AND source_id IN (SELECT id FROM delivery_items WHERE delivery_id=?1)", [document_id]).map_err(to_error)?;
            tx.execute(
                "DELETE FROM delivery_items WHERE delivery_id=?1",
                [document_id],
            )
            .map_err(to_error)?;
            for item in items {
                let order_item_id = int_value(item, "order_item_id");
                let (item_order_id, item_qty, item_batch): (i64, i64, String) = tx
                    .query_row(
                        "SELECT order_id,qty_grams,batch FROM order_items WHERE id=?1",
                        [order_item_id],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .map_err(|_| "送货明细对应的订单明细不存在".to_string())?;
                if item_order_id != order_id {
                    return Err("送货明细必须属于当前订单".into());
                }
                let qty = int_value(item, "qty_grams");
                if qty <= 0 || qty > item_qty {
                    return Err("送货数量必须大于 0 且不能超过订单明细数量".into());
                }
                let delivered_elsewhere: i64 = tx.query_row("SELECT COALESCE(SUM(qty_grams),0) FROM delivery_items WHERE order_item_id=?1", [order_item_id], |row| row.get(0)).map_err(to_error)?;
                if delivered_elsewhere + qty > item_qty {
                    return Err("累计送货数量不能超过订单明细数量".into());
                }
                let batch = if str_value(item, "batch").is_empty() {
                    item_batch
                } else {
                    str_value(item, "batch")
                };
                tx.execute("INSERT INTO delivery_items(delivery_id,order_item_id,batch,qty_grams) VALUES (?1,?2,?3,?4)", params![document_id, order_item_id, batch, qty]).map_err(to_error)?;
            }
            let sent: Option<String> = tx
                .query_row(
                    "SELECT sent_date FROM deliveries WHERE id=?1",
                    [document_id],
                    |row| row.get(0),
                )
                .map_err(to_error)?;
            if sent.is_some() {
                record_delivery_movements(tx, document_id)?;
            }
            Ok(tx
                .query_row(
                    "SELECT COALESCE(SUM(qty_grams),0) FROM delivery_items WHERE delivery_id=?1",
                    [document_id],
                    |row| row.get(0),
                )
                .map_err(to_error)?)
        }
        "statements" => {
            if !entity_exists(tx, "statements", document_id)? {
                return Err("对账单不存在".into());
            }
            let (customer_id, start, end): (i64, String, String) = tx
                .query_row(
                    "SELECT customer_id,period_start,period_end FROM statements WHERE id=?1",
                    [document_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(to_error)?;
            tx.execute(
                "DELETE FROM statement_items WHERE statement_id=?1",
                [document_id],
            )
            .map_err(to_error)?;
            for item in items {
                let order_id = int_value(item, "order_id");
                let allocated = if int_value(item, "allocated_amount_cents") > 0 {
                    int_value(item, "allocated_amount_cents")
                } else {
                    int_value(item, "amount_cents")
                };
                let (order_customer, delivery_date): (i64, Option<String>) = tx.query_row("SELECT customer_id,delivery_date FROM orders WHERE id=?1 AND archived_at IS NULL", [order_id], |row| Ok((row.get(0)?, row.get(1)?))).map_err(|_| "对账明细对应的订单不存在".to_string())?;
                if order_customer != customer_id {
                    return Err("对账明细客户必须与对账单一致".into());
                }
                if let Some(date) = delivery_date.as_deref() {
                    if date < start.as_str() || date > end.as_str() {
                        return Err("订单交期不在对账期间内".into());
                    }
                }
                let order_total = order_total_cents(tx, order_id)?;
                if allocated <= 0 || allocated > order_total {
                    return Err("对账分配金额必须大于 0 且不超过订单金额".into());
                }
                let other: i64 = tx.query_row("SELECT COALESCE(SUM(allocated_amount_cents),0) FROM statement_items WHERE order_id=?1 AND statement_id<>?2", params![order_id, document_id], |row| row.get(0)).map_err(to_error)?;
                if other + allocated > order_total {
                    return Err("同一订单的对账分配合计不能超过订单金额".into());
                }
                tx.execute("INSERT INTO statement_items(statement_id,order_id,allocated_amount_cents) VALUES (?1,?2,?3)", params![document_id, order_id, allocated]).map_err(to_error)?;
            }
            refresh_statement(tx, document_id)?;
            Ok(tx
                .query_row(
                    "SELECT total_cents FROM statements WHERE id=?1",
                    [document_id],
                    |row| row.get(0),
                )
                .map_err(to_error)?)
        }
        _ => Err(format!("不支持的明细单据类型：{document_type}")),
    }
}

fn record_delivery_movements(tx: &Transaction<'_>, delivery_id: i64) -> Result<(), String> {
    let order_id: i64 = tx
        .query_row(
            "SELECT order_id FROM deliveries WHERE id=?1",
            [delivery_id],
            |row| row.get(0),
        )
        .map_err(to_error)?;
    let lines: Vec<(i64, i64, i64, String)> = {
        let mut stmt = tx.prepare("SELECT di.id,oi.material_id,di.qty_grams,di.batch FROM delivery_items di JOIN order_items oi ON oi.id=di.order_item_id WHERE di.delivery_id=?1").map_err(to_error)?;
        let rows = stmt
            .query_map([delivery_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .map_err(to_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(to_error)?
    };
    if lines.is_empty() {
        return Err("送货单至少需要一条有效明细".into());
    }

    // Re-saving a dated delivery must validate the requested shipment against
    // stock before any new movement is written.  Remove this delivery's old
    // issues first, refresh the affected balances, and then aggregate by
    // material so multiple lines cannot each consume the same available stock.
    let old_materials: Vec<i64> = {
        let mut stmt = tx.prepare("SELECT DISTINCT material_id FROM inventory_movements WHERE movement_type='sale_issue' AND source_id IN (SELECT id FROM delivery_items WHERE delivery_id=?1)").map_err(to_error)?;
        let rows = stmt
            .query_map([delivery_id], |row| row.get(0))
            .map_err(to_error)?;
        rows.collect::<Result<Vec<i64>, _>>().map_err(to_error)?
    };
    tx.execute("DELETE FROM inventory_movements WHERE movement_type='sale_issue' AND source_id IN (SELECT id FROM delivery_items WHERE delivery_id=?1)", [delivery_id]).map_err(to_error)?;
    for material_id in old_materials {
        refresh_inventory(tx, material_id)?;
    }

    let mut requested_by_material = HashMap::<i64, i64>::new();
    for (_, material_id, qty, _) in &lines {
        if *qty <= 0 {
            return Err("送货数量必须大于 0".into());
        }
        let entry = requested_by_material.entry(*material_id).or_insert(0);
        *entry = entry
            .checked_add(*qty)
            .ok_or_else(|| "送货数量超出范围".to_string())?;
    }
    for (material_id, requested) in requested_by_material {
        let reservation: i64 = tx.query_row("SELECT COALESCE(SUM(reserved_grams),0) FROM stock_reservations WHERE order_id=?1 AND material_id=?2 AND status='active'", params![order_id, material_id], |row| row.get(0)).map_err(to_error)?;
        let available: i64 = tx.query_row("SELECT COALESCE(on_hand_grams+in_transit_grams-COALESCE((SELECT SUM(reserved_grams) FROM stock_reservations WHERE material_id=?1 AND status='active'),0),0) FROM inventory WHERE material_id=?1", [material_id], |row| row.get(0)).map_err(to_error)?;
        if requested > available.saturating_add(reservation) {
            return Err("库存可用量不足，不能登记送货出库".into());
        }
    }
    for (delivery_item_id, material_id, qty, batch) in lines {
        record_movement(
            tx,
            material_id,
            "sale_issue",
            qty,
            &batch,
            "",
            0,
            Some(delivery_item_id),
        )?;
    }
    tx.execute(
        "UPDATE stock_reservations SET status='released' WHERE order_id=?1 AND status='active'",
        [order_id],
    )
    .map_err(to_error)?;
    refresh_inventory_for_order(tx, order_id)?;
    Ok(())
}

fn refresh_inventory_for_order(tx: &Transaction<'_>, order_id: i64) -> Result<(), String> {
    let materials: Vec<i64> = {
        let mut stmt = tx.prepare("SELECT DISTINCT material_id FROM order_items WHERE order_id=?1 UNION SELECT material_id FROM orders WHERE id=?1").map_err(to_error)?;
        let rows = stmt
            .query_map([order_id], |row| row.get(0))
            .map_err(to_error)?;
        rows.collect::<Result<Vec<i64>, _>>().map_err(to_error)?
    };
    for material_id in materials {
        refresh_inventory(tx, material_id)?;
    }
    Ok(())
}

fn valid_status_value(entity: &str, value: &str) -> bool {
    match entity {
        "customers" | "projects" => matches!(
            value,
            "lead" | "contacted" | "sampling" | "quoting" | "won" | "paused_lost"
        ),
        "quotations" => matches!(
            value,
            "draft" | "sent" | "accepted" | "expired" | "rejected"
        ),
        "contracts" => matches!(value, "draft" | "signed" | "fulfilled" | "cancelled"),
        "samples" => matches!(
            value,
            "pending_send" | "sent" | "testing" | "passed" | "failed" | "retest"
        ),
        "orders" => matches!(
            value,
            "pending_confirm" | "preparing" | "delivered" | "signed" | "completed" | "cancelled"
        ),
        "purchases" => matches!(
            value,
            "pending_quote" | "ordered" | "in_transit" | "received" | "closed" | "cancelled"
        ),
        _ => true,
    }
}

fn status_for_save(
    tx: &Transaction<'_>,
    entity: &str,
    id: i64,
    provided: &str,
    default: &str,
    reason: Option<&str>,
) -> Result<String, String> {
    let current = if id > 0 {
        transition_field(entity).and_then(|field| {
            tx.query_row(
                &format!("SELECT {field} FROM {entity} WHERE id=?1"),
                [id],
                |row| row.get::<_, String>(0),
            )
            .ok()
        })
    } else {
        None
    };
    let next = if provided.is_empty() {
        current.clone().unwrap_or_else(|| default.to_string())
    } else {
        provided.to_string()
    };
    if !valid_status_value(entity, &next) {
        return Err(format!("{entity} 的状态值无效：{next}"));
    }
    if let Some(current) = current {
        if current != next {
            if !allowed_transition(entity, &current, &next) {
                return Err(format!(
                    "不允许从“{}”直接保存为“{}”，请使用状态流转",
                    current, next
                ));
            }
            if needs_reason(&current, &next) && reason.unwrap_or_default().trim().is_empty() {
                return Err("该状态变更必须通过状态流转并填写原因".into());
            }
        }
    }
    Ok(next)
}

fn save_record_tx(tx: &Transaction<'_>, entity: &str, data: &Value) -> Result<i64, String> {
    let id = int_value(data, "id");
    let now = now_iso();
    match entity {
        "customers" => {
            let name = required_string(data, "name", "客户名称")?;
            let stage =
                status_for_save(tx, "customers", id, &str_value(data, "stage"), "lead", None)?;
            let main_host = str_value(data, "main_host");
            let direction = str_value(data, "direction");
            let material_system = str_value(data, "material_system");
            let next_follow_date = optional_str(data, "next_follow_date");
            let notes = str_value(data, "notes");
            let result = if id > 0 {
                let changed = tx.execute("UPDATE customers SET name=?1,main_host=?2,direction=?3,material_system=?4,stage=?5,next_follow_date=?6,notes=?7,updated_at=?8 WHERE id=?9", params![name,main_host,direction,material_system,stage,next_follow_date,notes,now,id]).map_err(to_error)?;
                if changed > 0 {
                    id
                } else {
                    tx.execute("INSERT INTO customers(name,main_host,direction,material_system,stage,next_follow_date,notes,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![name,main_host,direction,material_system,stage,next_follow_date,notes,now,now]).map_err(to_error)?;
                    tx.last_insert_rowid()
                }
            } else {
                tx.execute("INSERT INTO customers(name,main_host,direction,material_system,stage,next_follow_date,notes,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![name,main_host,direction,material_system,stage,next_follow_date,notes,now,now]).map_err(to_error)?;
                tx.last_insert_rowid()
            };
            Ok(result)
        }
        "contacts" => {
            let name = required_string(data, "name", "联系人姓名")?;
            let customer_id = int_value(data, "customer_id");
            if customer_id <= 0 {
                return Err("客户 ID 必须为正数".into());
            }
            if id > 0 {
                let changed = tx.execute("UPDATE contacts SET customer_id=?1,name=?2,role=?3,phone=?4,email=?5,preferred_channel=?6,updated_at=?7 WHERE id=?8", params![customer_id,name,str_value(data,"role"),str_value(data,"phone"),str_value(data,"email"),str_value(data,"preferred_channel"),now,id]).map_err(to_error)?;
                if changed > 0 {
                    return Ok(id);
                }
            }
            tx.execute("INSERT INTO contacts(customer_id,name,role,phone,email,preferred_channel,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)", params![customer_id,name,str_value(data,"role"),str_value(data,"phone"),str_value(data,"email"),str_value(data,"preferred_channel"),now,now]).map_err(to_error)?;
            Ok(tx.last_insert_rowid())
        }
        "projects" => {
            let name = required_string(data, "name", "项目名称")?;
            let customer_id = int_value(data, "customer_id");
            if customer_id <= 0 {
                return Err("客户 ID 必须为正数".into());
            }
            let stage =
                status_for_save(tx, "projects", id, &str_value(data, "stage"), "lead", None)?;
            if id > 0 {
                let changed=tx.execute("UPDATE projects SET customer_id=?1,name=?2,main_host=?3,part_name=?4,expected_volume_grams=?5,stage=?6,next_follow_date=?7,updated_at=?8 WHERE id=?9",params![customer_id,name,str_value(data,"main_host"),str_value(data,"part_name"),int_value(data,"expected_volume_grams"),stage,optional_str(data,"next_follow_date"),now,id]).map_err(to_error)?;
                if changed > 0 {
                    return Ok(id);
                }
            }
            tx.execute("INSERT INTO projects(customer_id,name,main_host,part_name,expected_volume_grams,stage,next_follow_date,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![customer_id,name,str_value(data,"main_host"),str_value(data,"part_name"),int_value(data,"expected_volume_grams"),stage,optional_str(data,"next_follow_date"),now,now]).map_err(to_error)?;
            Ok(tx.last_insert_rowid())
        }
        "suppliers" => {
            let name = required_string(data, "name", "供应商名称")?;
            if id > 0 {
                let changed=tx.execute("UPDATE suppliers SET name=?1,contact=?2,price_ref_cents=?3,updated_at=?4 WHERE id=?5",params![name,str_value(data,"contact"),int_value(data,"price_ref_cents"),now,id]).map_err(to_error)?;
                if changed > 0 {
                    return Ok(id);
                }
            }
            tx.execute("INSERT INTO suppliers(name,contact,price_ref_cents,created_at,updated_at) VALUES (?1,?2,?3,?4,?5)",params![name,str_value(data,"contact"),int_value(data,"price_ref_cents"),now,now]).map_err(to_error)?;
            Ok(tx.last_insert_rowid())
        }
        "materials" => {
            let code = required_string(data, "code", "牌号")?;
            let base = required_string(data, "base_resin", "基材")?;
            let modification = str_value(data, "modification");
            let mi = str_value(data, "mi");
            let impact = str_value(data, "impact");
            let hdt = str_value(data, "hdt");
            let density = str_value(data, "density");
            let supplier_id = if int_value(data, "supplier_id") > 0 {
                Some(int_value(data, "supplier_id"))
            } else {
                None
            };
            let cost_cents = int_value(data, "cost_cents").max(0);
            let cost_date = optional_str(data, "cost_date");
            if id > 0 {
                let previous: Option<(i64, Option<String>)> = tx
                    .query_row(
                        "SELECT cost_cents,cost_date FROM materials WHERE id=?1",
                        [id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()
                    .map_err(to_error)?;
                let changed = tx.execute("UPDATE materials SET code=?1,base_resin=?2,modification=?3,mi=?4,impact=?5,hdt=?6,density=?7,supplier_id=?8,cost_cents=?9,cost_date=?10,updated_at=?11 WHERE id=?12", params![code, base, modification, mi, impact, hdt, density, supplier_id, cost_cents, cost_date, now, id]).map_err(to_error)?;
                if changed > 0 {
                    if previous
                        .as_ref()
                        .map(|(value, date)| *value != cost_cents || *date != cost_date)
                        .unwrap_or(true)
                    {
                        tx.execute("INSERT INTO cost_history(material_id,cost_cents,cost_date,source,note,created_at) VALUES (?1,?2,?3,'manual',?4,?5)", params![id, cost_cents, cost_date, "手工更新材料成本", now]).map_err(to_error)?;
                    }
                    return Ok(id);
                }
            }
            tx.execute("INSERT INTO materials(code,base_resin,modification,mi,impact,hdt,density,supplier_id,cost_cents,cost_date,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)", params![code, base, modification, mi, impact, hdt, density, supplier_id, cost_cents, cost_date, now, now]).map_err(to_error)?;
            let new_id = tx.last_insert_rowid();
            tx.execute("INSERT INTO cost_history(material_id,cost_cents,cost_date,source,note,created_at) VALUES (?1,?2,?3,'manual',?4,?5)", params![new_id, cost_cents, cost_date, "新增材料成本", now]).map_err(to_error)?;
            Ok(new_id)
        }
        "samples" => {
            let code = required_string(data, "code", "样品编号")?;
            let customer_id = int_value(data, "customer_id");
            let material_id = int_value(data, "material_id");
            if customer_id <= 0 || material_id <= 0 {
                return Err("客户 ID 和牌号 ID 必须为正数".into());
            }
            let fail_reason = str_value(data, "fail_reason");
            let status = status_for_save(
                tx,
                "samples",
                id,
                &str_value(data, "status"),
                "pending_send",
                Some(&fail_reason),
            )?;
            if matches!(status.as_str(), "failed" | "retest") && fail_reason.is_empty() {
                return Err("不通过或待复测时必须填写原因".into());
            }
            let saved_id = if id > 0 {
                let changed = tx.execute(
                    "UPDATE samples SET code=?1,customer_id=?2,material_id=?3,batch=?4,weight_grams=?5,sent_date=?6,test_items=?7,status=?8,fail_reason=?9,updated_at=?10 WHERE id=?11",
                    params![code, customer_id, material_id, str_value(data, "batch"), int_value(data, "weight_grams"), optional_str(data, "sent_date"), str_value(data, "test_items"), status, fail_reason, now, id],
                ).map_err(to_error)?;
                if changed > 0 {
                    id
                } else {
                    tx.execute(
                        "INSERT INTO samples(code,customer_id,material_id,batch,weight_grams,sent_date,test_items,status,fail_reason,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                        params![code, customer_id, material_id, str_value(data, "batch"), int_value(data, "weight_grams"), optional_str(data, "sent_date"), str_value(data, "test_items"), status, fail_reason, now, now],
                    ).map_err(to_error)?;
                    tx.last_insert_rowid()
                }
            } else {
                tx.execute(
                    "INSERT INTO samples(code,customer_id,material_id,batch,weight_grams,sent_date,test_items,status,fail_reason,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                    params![code, customer_id, material_id, str_value(data, "batch"), int_value(data, "weight_grams"), optional_str(data, "sent_date"), str_value(data, "test_items"), status, fail_reason, now, now],
                ).map_err(to_error)?;
                tx.last_insert_rowid()
            };
            validate_status_entry(tx, "samples", saved_id, &status)?;
            Ok(saved_id)
        }
        "follow_ups" => {
            let customer_id = int_value(data, "customer_id");
            if customer_id <= 0 {
                return Err("客户 ID 必须为正数".into());
            }
            let content = required_string(data, "content", "跟进内容")?;
            let follow_date = required_string(data, "follow_date", "跟进日期")?;
            let previous_customer_id = if id > 0 {
                tx.query_row(
                    "SELECT customer_id FROM follow_ups WHERE id=?1",
                    [id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(to_error)?
            } else {
                None
            };
            let saved_id = if id > 0 {
                let changed = tx.execute(
                    "UPDATE follow_ups SET customer_id=?1,content=?2,follow_date=?3,next_date=?4 WHERE id=?5",
                    params![customer_id, content, follow_date, optional_str(data, "next_date"), id],
                ).map_err(to_error)?;
                if changed > 0 {
                    id
                } else {
                    tx.execute(
                        "INSERT INTO follow_ups(customer_id,content,follow_date,next_date,created_at) VALUES (?1,?2,?3,?4,?5)",
                        params![customer_id, content, follow_date, optional_str(data, "next_date"), now],
                    ).map_err(to_error)?;
                    tx.last_insert_rowid()
                }
            } else {
                tx.execute(
                    "INSERT INTO follow_ups(customer_id,content,follow_date,next_date,created_at) VALUES (?1,?2,?3,?4,?5)",
                    params![customer_id, content, follow_date, optional_str(data, "next_date"), now],
                ).map_err(to_error)?;
                tx.last_insert_rowid()
            };
            if let Some(previous_customer_id) =
                previous_customer_id.filter(|value| *value != customer_id)
            {
                refresh_customer_next_follow_date(tx, previous_customer_id)?;
            }
            refresh_customer_next_follow_date(tx, customer_id)?;
            Ok(saved_id)
        }
        "sample_tests" => {
            let sample_id = int_value(data, "sample_id");
            if sample_id <= 0 {
                return Err("样品 ID 必须为正数".into());
            }
            let test_date = required_string(data, "test_date", "测试日期")?;
            let test_item = required_string(data, "test_item", "测试项目")?;
            if id > 0 {
                let changed = tx.execute(
                    "UPDATE sample_tests SET sample_id=?1,test_date=?2,test_item=?3,result=?4,conclusion=?5,next_action=?6 WHERE id=?7",
                    params![sample_id, test_date, test_item, str_value(data, "result"), str_value(data, "conclusion"), str_value(data, "next_action"), id],
                ).map_err(to_error)?;
                if changed > 0 {
                    return Ok(id);
                }
            }
            tx.execute(
                "INSERT INTO sample_tests(sample_id,test_date,test_item,result,conclusion,next_action,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![sample_id, test_date, test_item, str_value(data, "result"), str_value(data, "conclusion"), str_value(data, "next_action"), now],
            ).map_err(to_error)?;
            Ok(tx.last_insert_rowid())
        }
        "contracts" => {
            let no = required_string(data, "no", "合同号")?;
            let seller_name = required_string(data, "seller_name", "供方")?;
            let buyer_name = required_string(data, "buyer_name", "需方")?;
            let customer_id = int_value(data, "customer_id");
            let customer_id = if customer_id > 0 {
                if !entity_exists(tx, "customers", customer_id)? {
                    return Err("关联客户不存在".into());
                }
                Some(customer_id)
            } else {
                None
            };
            let status = status_for_save(
                tx,
                "contracts",
                id,
                &str_value(data, "status"),
                "draft",
                None,
            )?;
            let execution_place = str_value(data, "execution_place");
            let contract_date = optional_str(data, "contract_date");
            let settlement_method = str_value(data, "settlement_method");
            let packaging = str_value(data, "packaging");
            let terms = str_value(data, "terms");
            let seller_address = str_value(data, "seller_address");
            let seller_legal_representative = str_value(data, "seller_legal_representative");
            let seller_agent = str_value(data, "seller_agent");
            let seller_phone = str_value(data, "seller_phone");
            let seller_fax = str_value(data, "seller_fax");
            let seller_bank = str_value(data, "seller_bank");
            let seller_account = str_value(data, "seller_account");
            let buyer_address = str_value(data, "buyer_address");
            let buyer_legal_representative = str_value(data, "buyer_legal_representative");
            let buyer_agent = str_value(data, "buyer_agent");
            let buyer_phone = str_value(data, "buyer_phone");
            let buyer_fax = str_value(data, "buyer_fax");
            let buyer_bank = str_value(data, "buyer_bank");
            let buyer_account = str_value(data, "buyer_account");
            let saved_id = if id > 0 {
                let changed = tx.execute(
                    "UPDATE contracts SET no=?1,seller_name=?2,buyer_name=?3,customer_id=?4,execution_place=?5,contract_date=?6,settlement_method=?7,packaging=?8,terms=?9,seller_address=?10,seller_legal_representative=?11,seller_agent=?12,seller_phone=?13,seller_fax=?14,seller_bank=?15,seller_account=?16,buyer_address=?17,buyer_legal_representative=?18,buyer_agent=?19,buyer_phone=?20,buyer_fax=?21,buyer_bank=?22,buyer_account=?23,status=?24,updated_at=?25 WHERE id=?26",
                    params![no, seller_name, buyer_name, customer_id, execution_place, contract_date, settlement_method, packaging, terms, seller_address, seller_legal_representative, seller_agent, seller_phone, seller_fax, seller_bank, seller_account, buyer_address, buyer_legal_representative, buyer_agent, buyer_phone, buyer_fax, buyer_bank, buyer_account, status, now, id],
                ).map_err(to_error)?;
                if changed > 0 {
                    id
                } else {
                    tx.execute(
                        "INSERT INTO contracts(no,seller_name,buyer_name,customer_id,execution_place,contract_date,settlement_method,packaging,terms,seller_address,seller_legal_representative,seller_agent,seller_phone,seller_fax,seller_bank,seller_account,buyer_address,buyer_legal_representative,buyer_agent,buyer_phone,buyer_fax,buyer_bank,buyer_account,status,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26)",
                        params![no, seller_name, buyer_name, customer_id, execution_place, contract_date, settlement_method, packaging, terms, seller_address, seller_legal_representative, seller_agent, seller_phone, seller_fax, seller_bank, seller_account, buyer_address, buyer_legal_representative, buyer_agent, buyer_phone, buyer_fax, buyer_bank, buyer_account, status, now, now],
                    ).map_err(to_error)?;
                    tx.last_insert_rowid()
                }
            } else {
                tx.execute(
                    "INSERT INTO contracts(no,seller_name,buyer_name,customer_id,execution_place,contract_date,settlement_method,packaging,terms,seller_address,seller_legal_representative,seller_agent,seller_phone,seller_fax,seller_bank,seller_account,buyer_address,buyer_legal_representative,buyer_agent,buyer_phone,buyer_fax,buyer_bank,buyer_account,status,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26)",
                    params![no, seller_name, buyer_name, customer_id, execution_place, contract_date, settlement_method, packaging, terms, seller_address, seller_legal_representative, seller_agent, seller_phone, seller_fax, seller_bank, seller_account, buyer_address, buyer_legal_representative, buyer_agent, buyer_phone, buyer_fax, buyer_bank, buyer_account, status, now, now],
                ).map_err(to_error)?;
                tx.last_insert_rowid()
            };
            let items = item_values(data);
            if !items.is_empty() {
                save_document_items_tx(tx, "contracts", saved_id, &items)?;
            }
            Ok(saved_id)
        }
        "quotations" => {
            let no = required_string(data, "no", "报价单号")?;
            let customer_id = int_value(data, "customer_id");
            let material_id = int_value(data, "material_id");
            let price = int_value(data, "price_cents");
            if customer_id <= 0 || material_id <= 0 {
                return Err("客户 ID 和牌号 ID 必须为正数".into());
            }
            if price <= 0 {
                return Err("含税单价必须大于 0".into());
            }
            let status = status_for_save(
                tx,
                "quotations",
                id,
                &str_value(data, "status"),
                "draft",
                None,
            )?;
            let version = if int_value(data, "version") > 0 {
                int_value(data, "version")
            } else {
                1
            };
            let seller_name = str_value(data, "seller_name");
            let recipient_name = str_value(data, "recipient_name");
            let sender_name = str_value(data, "sender_name");
            let recipient_contact = str_value(data, "recipient_contact");
            let recipient_fax = str_value(data, "recipient_fax");
            let cc = str_value(data, "cc");
            let page_count = int_value(data, "page_count").max(1);
            let request_review = bool_int_value(data, "request_review", true);
            let request_comment = bool_int_value(data, "request_comment", true);
            let quote_date = optional_str(data, "quote_date");
            let subject = str_value(data, "subject");
            let price_note = str_value(data, "price_note");
            let adjustment_note = str_value(data, "adjustment_note");
            let footer_address = str_value(data, "footer_address");
            let footer_phone = str_value(data, "footer_phone");
            let footer_fax = str_value(data, "footer_fax");
            let footer_email = str_value(data, "footer_email");
            let saved_id = if id > 0 {
                let changed = tx.execute(
                    "UPDATE quotations SET no=?1,customer_id=?2,material_id=?3,price_cents=?4,moq_grams=?5,freight=?6,valid_until=?7,seller_name=?8,recipient_name=?9,sender_name=?10,recipient_contact=?11,recipient_fax=?12,cc=?13,page_count=?14,request_review=?15,request_comment=?16,quote_date=?17,subject=?18,price_note=?19,adjustment_note=?20,footer_address=?21,footer_phone=?22,footer_fax=?23,footer_email=?24,version=?25,status=?26,updated_at=?27 WHERE id=?28",
                    params![no, customer_id, material_id, price, int_value(data, "moq_grams"), str_value(data, "freight"), optional_str(data, "valid_until"), seller_name, recipient_name, sender_name, recipient_contact, recipient_fax, cc, page_count, request_review, request_comment, quote_date, subject, price_note, adjustment_note, footer_address, footer_phone, footer_fax, footer_email, version, status, now, id],
                ).map_err(to_error)?;
                if changed > 0 {
                    id
                } else {
                    tx.execute(
                        "INSERT INTO quotations(no,customer_id,material_id,price_cents,moq_grams,freight,valid_until,seller_name,recipient_name,sender_name,recipient_contact,recipient_fax,cc,page_count,request_review,request_comment,quote_date,subject,price_note,adjustment_note,footer_address,footer_phone,footer_fax,footer_email,version,status,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28)",
                        params![no, customer_id, material_id, price, int_value(data, "moq_grams"), str_value(data, "freight"), optional_str(data, "valid_until"), seller_name, recipient_name, sender_name, recipient_contact, recipient_fax, cc, page_count, request_review, request_comment, quote_date, subject, price_note, adjustment_note, footer_address, footer_phone, footer_fax, footer_email, version, status, now, now],
                    ).map_err(to_error)?;
                    tx.last_insert_rowid()
                }
            } else {
                tx.execute(
                    "INSERT INTO quotations(no,customer_id,material_id,price_cents,moq_grams,freight,valid_until,seller_name,recipient_name,sender_name,recipient_contact,recipient_fax,cc,page_count,request_review,request_comment,quote_date,subject,price_note,adjustment_note,footer_address,footer_phone,footer_fax,footer_email,version,status,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28)",
                    params![no, customer_id, material_id, price, int_value(data, "moq_grams"), str_value(data, "freight"), optional_str(data, "valid_until"), seller_name, recipient_name, sender_name, recipient_contact, recipient_fax, cc, page_count, request_review, request_comment, quote_date, subject, price_note, adjustment_note, footer_address, footer_phone, footer_fax, footer_email, version, status, now, now],
                ).map_err(to_error)?;
                tx.last_insert_rowid()
            };
            let items = item_values(data);
            if !items.is_empty() {
                save_document_items_tx(tx, "quotations", saved_id, &items)?;
            }
            Ok(saved_id)
        }
        "orders" => {
            let no = required_string(data, "no", "订单号")?;
            let customer_id = int_value(data, "customer_id");
            let material_id = int_value(data, "material_id");
            let qty = int_value(data, "qty_grams");
            let price = int_value(data, "price_cents");
            if customer_id <= 0 || material_id <= 0 {
                return Err("客户 ID 和牌号 ID 必须为正数".into());
            }
            if qty <= 0 || price <= 0 {
                return Err("订单数量和单价必须大于 0".into());
            }
            let status = status_for_save(
                tx,
                "orders",
                id,
                &str_value(data, "status"),
                "pending_confirm",
                None,
            )?;
            let sign_status = if str_value(data, "sign_status").is_empty() {
                "pending".to_string()
            } else {
                str_value(data, "sign_status")
            };
            let quotation_id = if int_value(data, "quotation_id") > 0 {
                Some(int_value(data, "quotation_id"))
            } else {
                None
            };
            let saved_id = if id > 0 {
                let changed = tx.execute("UPDATE orders SET no=?1,customer_id=?2,material_id=?3,qty_grams=?4,price_cents=?5,delivery_date=?6,status=?7,sign_status=?8,quotation_id=?9,updated_at=?10 WHERE id=?11", params![no, customer_id, material_id, qty, price, optional_str(data, "delivery_date"), status, sign_status, quotation_id, now, id]).map_err(to_error)?;
                if changed > 0 {
                    id
                } else {
                    tx.execute("INSERT INTO orders(no,customer_id,material_id,qty_grams,price_cents,delivery_date,status,sign_status,quotation_id,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)", params![no, customer_id, material_id, qty, price, optional_str(data, "delivery_date"), status, sign_status, quotation_id, now, now]).map_err(to_error)?;
                    tx.last_insert_rowid()
                }
            } else {
                tx.execute("INSERT INTO orders(no,customer_id,material_id,qty_grams,price_cents,delivery_date,status,sign_status,quotation_id,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)", params![no, customer_id, material_id, qty, price, optional_str(data, "delivery_date"), status, sign_status, quotation_id, now, now]).map_err(to_error)?;
                tx.last_insert_rowid()
            };
            let items = item_values(data);
            if !items.is_empty() {
                save_document_items_tx(tx, "orders", saved_id, &items)?;
            }
            refresh_order_reservation(tx, saved_id)?;
            refresh_statements_for_order(tx, saved_id)?;
            validate_status_entry(tx, "orders", saved_id, &status)?;
            Ok(saved_id)
        }
        "deliveries" => {
            let no = required_string(data, "no", "送货单号")?;
            let order_id = int_value(data, "order_id");
            if order_id <= 0 || !entity_exists(tx, "orders", order_id)? {
                return Err("订单 ID 必须为正数且订单必须存在".into());
            }
            let sent_date = optional_str(data, "sent_date");
            let sign_date = optional_str(data, "sign_date");
            let saved_id = if id > 0 {
                let changed = tx.execute("UPDATE deliveries SET no=?1,order_id=?2,sent_date=?3,sign_date=?4,updated_at=?5 WHERE id=?6", params![no, order_id, sent_date, sign_date, now, id]).map_err(to_error)?;
                if changed > 0 {
                    id
                } else {
                    tx.execute("INSERT INTO deliveries(no,order_id,sent_date,sign_date,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6)", params![no, order_id, sent_date, sign_date, now, now]).map_err(to_error)?;
                    tx.last_insert_rowid()
                }
            } else {
                tx.execute("INSERT INTO deliveries(no,order_id,sent_date,sign_date,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6)", params![no, order_id, sent_date, sign_date, now, now]).map_err(to_error)?;
                tx.last_insert_rowid()
            };
            let items = item_values(data);
            if !items.is_empty() {
                save_document_items_tx(tx, "deliveries", saved_id, &items)?;
            }
            if sent_date.is_some() {
                ensure_delivery_items_from_order(tx, saved_id)?;
                record_delivery_movements(tx, saved_id)?;
                if sign_date.is_some() {
                    tx.execute("UPDATE orders SET status='signed',sign_status='signed',updated_at=?1 WHERE id=?2 AND status IN ('preparing','delivered','signed')", params![now_iso(), order_id]).map_err(to_error)?;
                } else {
                    tx.execute("UPDATE orders SET status='delivered',updated_at=?1 WHERE id=?2 AND status IN ('preparing','pending_confirm')", params![now_iso(), order_id]).map_err(to_error)?;
                }
            } else if sign_date.is_some() {
                return Err("登记回签前必须填写送货日期".into());
            }
            Ok(saved_id)
        }
        "statements" => {
            let no = required_string(data, "no", "对账单号")?;
            let customer_id = int_value(data, "customer_id");
            let period_start = required_string(data, "period_start", "对账期间开始")?;
            let period_end = required_string(data, "period_end", "对账期间结束")?;
            if customer_id <= 0 {
                return Err("客户 ID 必须为正数".into());
            }
            if period_start > period_end {
                return Err("对账期间开始不能晚于结束".into());
            }
            let saved_id = if id > 0 {
                let changed = tx.execute("UPDATE statements SET no=?1,customer_id=?2,period_start=?3,period_end=?4,account_days=?5,promise_date=?6,updated_at=?7 WHERE id=?8", params![no, customer_id, period_start, period_end, int_value(data, "account_days"), optional_str(data, "promise_date"), now, id]).map_err(to_error)?;
                if changed > 0 {
                    id
                } else {
                    tx.execute("INSERT INTO statements(no,customer_id,period_start,period_end,account_days,promise_date,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)", params![no, customer_id, period_start, period_end, int_value(data, "account_days"), optional_str(data, "promise_date"), now, now]).map_err(to_error)?;
                    tx.last_insert_rowid()
                }
            } else {
                tx.execute("INSERT INTO statements(no,customer_id,period_start,period_end,account_days,promise_date,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)", params![no, customer_id, period_start, period_end, int_value(data, "account_days"), optional_str(data, "promise_date"), now, now]).map_err(to_error)?;
                tx.last_insert_rowid()
            };
            let items = item_values(data);
            if !items.is_empty() {
                save_document_items_tx(tx, "statements", saved_id, &items)?;
            } else {
                refresh_statement(tx, saved_id)?;
            }
            Ok(saved_id)
        }
        "payments" => {
            let statement_id = int_value(data, "statement_id");
            let amount = int_value(data, "amount_cents");
            let pay_date = required_string(data, "pay_date", "回款日期")?;
            if statement_id <= 0 || !entity_exists(tx, "statements", statement_id)? {
                return Err("对账单 ID 必须为正数且对账单必须存在".into());
            }
            if amount <= 0 {
                return Err("回款金额必须大于 0".into());
            }
            let saved_id = if id > 0 {
                let changed = tx.execute("UPDATE payments SET statement_id=?1,amount_cents=?2,pay_date=?3,method=?4 WHERE id=?5", params![statement_id, amount, pay_date, str_value(data, "method"), id]).map_err(to_error)?;
                if changed > 0 {
                    id
                } else {
                    tx.execute("INSERT INTO payments(statement_id,amount_cents,pay_date,method,created_at) VALUES (?1,?2,?3,?4,?5)", params![statement_id, amount, pay_date, str_value(data, "method"), now]).map_err(to_error)?;
                    tx.last_insert_rowid()
                }
            } else {
                tx.execute("INSERT INTO payments(statement_id,amount_cents,pay_date,method,created_at) VALUES (?1,?2,?3,?4,?5)", params![statement_id, amount, pay_date, str_value(data, "method"), now]).map_err(to_error)?;
                tx.last_insert_rowid()
            };
            let allocations = item_values(data);
            let allocations = if allocations.is_empty() {
                vec![json!({"statement_id": statement_id, "allocated_amount_cents": amount})]
            } else {
                allocations
            };
            let statement_ids = replace_payment_allocations(tx, saved_id, &allocations)?;
            for affected in statement_ids {
                refresh_statement(tx, affected)?;
            }
            Ok(saved_id)
        }
        "purchases" => {
            let no = required_string(data, "no", "采购单号")?;
            let supplier_id = int_value(data, "supplier_id");
            let material_id = int_value(data, "material_id");
            let qty = int_value(data, "qty_grams");
            if supplier_id <= 0 || material_id <= 0 {
                return Err("供应商 ID 和牌号 ID 必须为正数".into());
            }
            if qty <= 0 {
                return Err("采购量必须大于 0".into());
            }
            let status = status_for_save(
                tx,
                "purchases",
                id,
                &str_value(data, "status"),
                "pending_quote",
                None,
            )?;
            let previous: Option<(i64, String)> = if id > 0 {
                tx.query_row(
                    "SELECT material_id,status FROM purchases WHERE id=?1",
                    [id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(to_error)?
            } else {
                None
            };
            let saved_id = if id > 0 {
                let changed = tx.execute("UPDATE purchases SET no=?1,supplier_id=?2,material_id=?3,qty_grams=?4,status=?5,eta=?6,updated_at=?7 WHERE id=?8", params![no, supplier_id, material_id, qty, status, optional_str(data, "eta"), now, id]).map_err(to_error)?;
                if changed > 0 {
                    id
                } else {
                    tx.execute("INSERT INTO purchases(no,supplier_id,material_id,qty_grams,status,eta,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)", params![no, supplier_id, material_id, qty, status, optional_str(data, "eta"), now, now]).map_err(to_error)?;
                    tx.last_insert_rowid()
                }
            } else {
                tx.execute("INSERT INTO purchases(no,supplier_id,material_id,qty_grams,status,eta,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)", params![no, supplier_id, material_id, qty, status, optional_str(data, "eta"), now, now]).map_err(to_error)?;
                tx.last_insert_rowid()
            };
            if status == "received" {
                record_purchase_receipt(tx, saved_id)?;
            }
            refresh_inventory_transit(tx, material_id)?;
            if let Some((old_material, _)) = previous.filter(|(old, _)| *old != material_id) {
                refresh_inventory_transit(tx, old_material)?;
            }
            Ok(saved_id)
        }
        "inventory" => {
            let material_id = int_value(data, "material_id");
            if material_id <= 0 || !entity_exists(tx, "materials", material_id)? {
                return Err("牌号 ID 必须为正数且牌号必须存在".into());
            }
            let on_hand = int_value(data, "on_hand_grams").max(0);
            let safety = int_value(data, "safety_grams").max(0);
            let movement_sum: i64 = tx.query_row("SELECT COALESCE(SUM(CASE WHEN movement_type IN ('opening','purchase_receipt','adjustment_in','return_in') THEN qty_grams ELSE -qty_grams END),0) FROM inventory_movements WHERE material_id=?1", [material_id], |row| row.get(0)).map_err(to_error)?;
            let opening = on_hand.saturating_sub(movement_sum);
            let saved_id = if id > 0 {
                let changed = tx.execute("UPDATE inventory SET material_id=?1,opening_grams=?2,safety_grams=?3,updated_at=?4 WHERE id=?5", params![material_id, opening, safety, now, id]).map_err(to_error)?;
                if changed > 0 {
                    id
                } else {
                    tx.execute("INSERT INTO inventory(material_id,opening_grams,on_hand_grams,in_transit_grams,safety_grams,updated_at) VALUES (?1,?2,?3,0,?4,?5) ON CONFLICT(material_id) DO UPDATE SET opening_grams=excluded.opening_grams,safety_grams=excluded.safety_grams,updated_at=excluded.updated_at", params![material_id, opening, on_hand, safety, now]).map_err(to_error)?;
                    tx.query_row(
                        "SELECT id FROM inventory WHERE material_id=?1",
                        [material_id],
                        |row| row.get(0),
                    )
                    .map_err(to_error)?
                }
            } else {
                tx.execute("INSERT INTO inventory(material_id,opening_grams,on_hand_grams,in_transit_grams,safety_grams,updated_at) VALUES (?1,?2,?3,0,?4,?5) ON CONFLICT(material_id) DO UPDATE SET opening_grams=excluded.opening_grams,safety_grams=excluded.safety_grams,updated_at=excluded.updated_at", params![material_id, opening, on_hand, safety, now]).map_err(to_error)?;
                tx.query_row(
                    "SELECT id FROM inventory WHERE material_id=?1",
                    [material_id],
                    |row| row.get(0),
                )
                .map_err(to_error)?
            };
            refresh_inventory(tx, material_id)?;
            Ok(saved_id)
        }
        "inventory_movements" => {
            let material_id = int_value(data, "material_id");
            let movement_type = required_string(data, "movement_type", "流水类型")?;
            let qty = int_value(data, "qty_grams");
            let id = record_movement(
                tx,
                material_id,
                &movement_type,
                qty,
                &str_value(data, "batch"),
                &str_value(data, "warehouse"),
                int_value(data, "unit_cost_cents"),
                if int_value(data, "source_id") > 0 {
                    Some(int_value(data, "source_id"))
                } else {
                    None
                },
            )?;
            Ok(id)
        }
        "stock_reservations" => {
            let material_id = int_value(data, "material_id");
            if material_id <= 0 || !entity_exists(tx, "materials", material_id)? {
                return Err("牌号必须存在".into());
            }
            let order_id = if int_value(data, "order_id") > 0 {
                Some(int_value(data, "order_id"))
            } else {
                None
            };
            if order_id.is_some() && !entity_exists(tx, "orders", order_id.unwrap_or_default())? {
                return Err("订单不存在".into());
            }
            let reserved = int_value(data, "reserved_grams");
            if reserved <= 0 {
                return Err("预留数量必须大于 0".into());
            }
            let status = if str_value(data, "status").is_empty() {
                "active".to_string()
            } else {
                str_value(data, "status")
            };
            if !matches!(status.as_str(), "active" | "released" | "cancelled") {
                return Err("预留状态无效".into());
            }
            if id > 0 {
                let changed = tx.execute("UPDATE stock_reservations SET material_id=?1,order_id=?2,reserved_grams=?3,batch=?4,warehouse=?5,status=?6 WHERE id=?7", params![material_id, order_id, reserved, str_value(data, "batch"), str_value(data, "warehouse"), status, id]).map_err(to_error)?;
                if changed > 0 {
                    refresh_inventory(tx, material_id)?;
                    return Ok(id);
                }
            }
            tx.execute("INSERT INTO stock_reservations(material_id,order_id,reserved_grams,batch,warehouse,status,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)", params![material_id, order_id, reserved, str_value(data, "batch"), str_value(data, "warehouse"), status, now]).map_err(to_error)?;
            refresh_inventory(tx, material_id)?;
            Ok(tx.last_insert_rowid())
        }
        "attachments" => {
            let object_type = validate_attachment_object_type(&required_string(
                data,
                "object_type",
                "对象类型",
            )?)?;
            let object_id = int_value(data, "object_id");
            if object_id <= 0 {
                return Err("对象 ID 必须为正数".into());
            }
            if !entity_exists(tx, &object_type, object_id)? {
                return Err("附件关联对象不存在".into());
            }
            let relative_path =
                validate_relative_attachment_path(&str_value(data, "relative_path"))?;
            let hash = validate_hash(&str_value(data, "hash"))?;
            if id > 0 {
                let changed = tx.execute("UPDATE attachments SET object_type=?1,object_id=?2,relative_path=?3,hash=?4 WHERE id=?5", params![object_type, object_id, relative_path, hash, id]).map_err(to_error)?;
                if changed > 0 {
                    return Ok(id);
                }
            }
            tx.execute("INSERT INTO attachments(object_type,object_id,relative_path,hash,created_at) VALUES (?1,?2,?3,?4,?5)", params![object_type, object_id, relative_path, hash, now]).map_err(to_error)?;
            Ok(tx.last_insert_rowid())
        }
        "cost_history" => {
            let material_id = int_value(data, "material_id");
            if material_id <= 0 || !entity_exists(tx, "materials", material_id)? {
                return Err("牌号必须存在".into());
            }
            let cost = int_value(data, "cost_cents");
            if cost < 0 {
                return Err("成本不能为负数".into());
            }
            let source = str_value(data, "source");
            let source = if source.is_empty() {
                "manual"
            } else {
                source.as_str()
            };
            tx.execute("INSERT INTO cost_history(material_id,cost_cents,cost_date,source,note,created_at) VALUES (?1,?2,?3,?4,?5,?6)", params![material_id, cost, optional_str(data, "cost_date"), source, str_value(data, "note"), now]).map_err(to_error)?;
            Ok(tx.last_insert_rowid())
        }
        _ => Err(format!("不支持的业务对象：{entity}")),
    }
}

fn refresh_inventory_transit(tx: &Transaction<'_>, material_id: i64) -> Result<(), String> {
    refresh_inventory(tx, material_id)
}

fn allowed_transition(entity: &str, current: &str, next: &str) -> bool {
    if current == next {
        return true;
    }
    match entity {
        "customers" | "projects" => matches!(
            (current, next),
            ("lead", "contacted")
                | ("contacted", "sampling")
                | ("contacted", "paused_lost")
                | ("sampling", "quoting")
                | ("sampling", "paused_lost")
                | ("quoting", "won")
                | ("quoting", "paused_lost")
                | ("won", "paused_lost")
                | ("paused_lost", "contacted")
                | ("paused_lost", "sampling")
                | ("paused_lost", "quoting")
                | ("paused_lost", "won")
        ),
        "quotations" => matches!(
            (current, next),
            ("draft", "sent")
                | ("sent", "accepted")
                | ("sent", "expired")
                | ("sent", "rejected")
                | ("draft", "rejected")
                | ("accepted", "accepted")
                | ("expired", "expired")
                | ("rejected", "rejected")
        ),
        "contracts" => matches!(
            (current, next),
            ("draft", "signed")
                | ("draft", "cancelled")
                | ("signed", "fulfilled")
                | ("signed", "cancelled")
                | ("fulfilled", "fulfilled")
                | ("cancelled", "cancelled")
        ),
        "samples" => matches!(
            (current, next),
            ("pending_send", "sent")
                | ("sent", "testing")
                | ("testing", "passed")
                | ("testing", "failed")
                | ("testing", "retest")
                | ("failed", "retest")
                | ("failed", "failed")
                | ("retest", "sent")
                | ("retest", "testing")
                | ("passed", "passed")
        ),
        "orders" => matches!(
            (current, next),
            ("pending_confirm", "preparing")
                | ("pending_confirm", "cancelled")
                | ("preparing", "delivered")
                | ("preparing", "cancelled")
                | ("delivered", "signed")
                | ("signed", "completed")
                | ("completed", "completed")
                | ("cancelled", "cancelled")
        ),
        "purchases" => matches!(
            (current, next),
            ("pending_quote", "ordered")
                | ("pending_quote", "cancelled")
                | ("ordered", "in_transit")
                | ("ordered", "cancelled")
                | ("in_transit", "received")
                | ("received", "closed")
                | ("closed", "closed")
                | ("cancelled", "cancelled")
        ),
        "statements" => matches!(
            (current, next),
            ("not_due", "near_due")
                | ("not_due", "overdue")
                | ("not_due", "settled")
                | ("near_due", "overdue")
                | ("near_due", "settled")
                | ("overdue", "settled")
                | ("settled", "settled")
        ),
        _ => false,
    }
}

fn transition_field(entity: &str) -> Option<&'static str> {
    match entity {
        "customers" | "projects" => Some("stage"),
        "quotations" | "contracts" => Some("status"),
        "statements" => Some("pay_status"),
        "samples" | "orders" | "purchases" => Some("status"),
        _ => None,
    }
}

fn needs_reason(current: &str, next: &str) -> bool {
    matches!(
        next,
        "paused_lost" | "failed" | "retest" | "cancelled" | "rejected"
    ) || (current == "won" && next == "paused_lost")
}

fn validate_status_entry(
    tx: &Transaction<'_>,
    entity: &str,
    id: i64,
    next: &str,
) -> Result<(), String> {
    match entity {
        "statements" => {
            Err("对账状态由应收余额、承诺回款日和提醒参数自动计算，不能手工流转".into())
        }
        "samples" => {
            let (batch, sent_date, test_items, fail_reason): (
                String,
                Option<String>,
                String,
                String,
            ) = tx
                .query_row(
                    "SELECT batch,sent_date,test_items,fail_reason FROM samples WHERE id=?1",
                    [id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .map_err(to_error)?;
            match next {
                "sent" => {
                    if batch.trim().is_empty()
                        || sent_date.as_deref().unwrap_or_default().trim().is_empty()
                    {
                        return Err("样品进入已送样前必须填写送样日期和批次".into());
                    }
                }
                "testing" => {
                    let has_test_item: i64 = tx.query_row("SELECT EXISTS(SELECT 1 FROM sample_tests WHERE sample_id=?1 AND TRIM(test_item)<>'')", [id], |row| row.get(0)).map_err(to_error)?;
                    if test_items.trim().is_empty() && has_test_item == 0 {
                        return Err("样品进入测试中前必须填写测试项目".into());
                    }
                }
                "passed" => {
                    let conclusion: Option<String> = tx.query_row("SELECT conclusion FROM sample_tests WHERE sample_id=?1 ORDER BY test_date DESC,id DESC LIMIT 1", [id], |row| row.get(0)).optional().map_err(to_error)?.flatten();
                    if conclusion.as_deref().unwrap_or_default().trim().is_empty() {
                        return Err("样品测试通过前必须登记测试结论".into());
                    }
                }
                "failed" | "retest" => {
                    if fail_reason.trim().is_empty() {
                        return Err("样品不通过或待复测前必须填写原因".into());
                    }
                }
                _ => {}
            }
            Ok(())
        }
        "orders" => {
            match next {
                "preparing" => {
                    let delivery_date: Option<String> = tx
                        .query_row(
                            "SELECT delivery_date FROM orders WHERE id=?1",
                            [id],
                            |row| row.get(0),
                        )
                        .map_err(to_error)?;
                    if delivery_date
                        .as_deref()
                        .unwrap_or_default()
                        .trim()
                        .is_empty()
                    {
                        return Err("订单进入备货中前必须填写交期".into());
                    }
                }
                "delivered" => {
                    let delivered: i64 = tx.query_row("SELECT EXISTS(SELECT 1 FROM deliveries d WHERE d.order_id=?1 AND d.sent_date IS NOT NULL AND EXISTS (SELECT 1 FROM delivery_items di WHERE di.delivery_id=d.id AND di.qty_grams>0))", [id], |row| row.get(0)).map_err(to_error)?;
                    if delivered == 0 {
                        return Err("订单进入已送货前必须登记送货日期和数量".into());
                    }
                }
                "signed" => {
                    let signed: i64 = tx.query_row("SELECT EXISTS(SELECT 1 FROM deliveries d WHERE d.order_id=?1 AND d.sent_date IS NOT NULL AND d.sign_date IS NOT NULL)", [id], |row| row.get(0)).map_err(to_error)?;
                    if signed == 0 {
                        return Err("订单进入已回签前必须登记送货日期和回签日期".into());
                    }
                }
                _ => {}
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn backup_database(
    conn: &Connection,
    root: &Path,
    master: Option<&[u8; 32]>,
) -> Result<BackupResult, String> {
    let stamp = Local::now().format("%Y%m%d-%H%M%S%.3f").to_string();
    let protected = master.is_some();
    let backup_root = root.join("backup");
    fs::create_dir_all(&backup_root).map_err(|error| {
        format!(
            "备份目录创建失败：{}。请检查磁盘空间和目录权限后重试。",
            file_operation_error(&backup_root, error)
        )
    })?;
    let suffix = if protected { ".db.enc" } else { ".db" };
    let path = unique_backup_path(&backup_root, &stamp, suffix)?;
    let snapshot = backup_root.join(format!(
        ".erp-{stamp}-{}.db.snapshot",
        hex::encode(random_bytes::<8>())
    ));
    backup_database_to_paths(conn, root, master, &snapshot, &path)
}

fn unique_backup_path(backup_root: &Path, stamp: &str, suffix: &str) -> Result<PathBuf, String> {
    let first = backup_root.join(format!("erp-{stamp}{suffix}"));
    if !first.exists() {
        return Ok(first);
    }
    for _ in 0..16 {
        let candidate = backup_root.join(format!(
            "erp-{stamp}-{}{suffix}",
            hex::encode(random_bytes::<8>())
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "备份目标文件名冲突，未覆盖既有文件：{}。请稍后重试。",
        first.display()
    ))
}

fn backup_database_to_paths(
    conn: &Connection,
    root: &Path,
    master: Option<&[u8; 32]>,
    snapshot: &Path,
    path: &Path,
) -> Result<BackupResult, String> {
    // The snapshot and final file are both attempt-scoped. A failed attempt
    // must not leave a stale SQLite snapshot or a partially written backup.
    remove_file_if_exists(snapshot).map_err(|error| {
        format!("备份前清理临时快照失败：{error}。请检查磁盘空间、目录权限或占用程序后重试。")
    })?;
    let snapshot_string = snapshot.to_string_lossy().to_string();
    let result = (|| {
        conn.execute("VACUUM INTO ?1", [&snapshot_string])
            .map_err(|error| {
                format!(
                    "备份快照创建失败：{}。请检查磁盘空间、目录权限或占用程序后重试。",
                    database_file_error(snapshot, error)
                )
            })?;
        let plain_bytes = fs::read(snapshot).map_err(|error| {
            format!(
                "备份快照读取失败：{}。请检查磁盘空间、目录权限或占用程序后重试。",
                file_operation_error(snapshot, error)
            )
        })?;
        remove_file_if_exists(snapshot)
            .map_err(|error| format!("备份临时快照清理失败：{error}。请关闭占用程序后重试。"))?;
        let bytes = if let Some(master) = master {
            encrypt_bytes(master, &plain_bytes)
                .map_err(|error| format!("备份加密失败：{error}。请重试。"))?
        } else {
            plain_bytes
        };
        if path.exists() {
            return Err(format!(
                "备份文件写入失败：目标已存在，未覆盖既有文件：{}。请重试。",
                path.display()
            ));
        }
        write_bytes_atomically(path, &bytes).map_err(|error| {
            format!("备份文件写入失败：{error}。请检查磁盘空间、目录权限或占用程序后重试。")
        })?;
        let path_string = path.to_string_lossy().to_string();
        let checksum = hex::encode(Sha256::digest(&bytes));
        let result = BackupResult {
            path: path_string,
            checksum,
            created_at: now_iso(),
        };
        if let Err(error) = conn.execute(
            "INSERT INTO backup_records(path,checksum,size_bytes,schema_version,result,created_at) VALUES (?1,?2,?3,?4,'success',?5)",
            params![result.path, result.checksum, bytes.len() as i64, SCHEMA_VERSION, result.created_at],
        ) {
            return Err(format!(
                "备份文件已生成但登记失败：{error}；文件已保留于 {}，请在备份列表核验后重试。",
                result.path
            ));
        }
        if let Err(error) = register_backup_manifest(
            root,
            BackupManifestEntry {
                path: result.path.clone(),
                checksum: result.checksum.clone(),
                size_bytes: bytes.len() as i64,
                schema_version: SCHEMA_VERSION,
                created_at: result.created_at.clone(),
            },
        ) {
            return Err(format!(
                "备份文件已生成且数据库已登记，但备份清单写入失败：{error}；文件已保留于 {}，请在备份列表核验后重试。",
                result.path
            ));
        }
        prune_backups(conn, root, &result.path);
        let _ = remove_missing_backup_manifest_entries(root);
        Ok(result)
    })();
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            if let Err(cleanup) = remove_file_if_exists(snapshot) {
                return Err(format!("{error}；临时快照清理失败：{cleanup}"));
            }
            Err(error)
        }
    }
}

fn prune_backups(conn: &Connection, root: &Path, newest: &str) {
    let keep: usize = conn
        .query_row(
            "SELECT value FROM settings WHERE key='backup_retention'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(10);
    let mut entries: Vec<(PathBuf, std::time::SystemTime)> = fs::read_dir(root.join("backup"))
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if !path.is_file() || path.to_string_lossy() == newest || !is_backup_file_name(&path) {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((path, modified))
        })
        .collect();
    entries.sort_by_key(|(_, modified)| *modified);
    let remove_count = entries.len().saturating_sub(keep.saturating_sub(1));
    for (path, _) in entries.into_iter().take(remove_count) {
        let path_string = path.to_string_lossy().to_string();
        let _ = fs::remove_file(&path);
        let _ = conn.execute("DELETE FROM backup_records WHERE path=?1", [path_string]);
    }
}

fn is_backup_file_name(path: &Path) -> bool {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    file_name.starts_with("erp-") && matches!(extension, "db" | "enc")
}

fn expected_backup_checksum(
    root: &Path,
    backup_path: &str,
    conn: Option<&Connection>,
) -> Option<String> {
    let database_value = conn.and_then(|conn| {
        conn.query_row(
            "SELECT checksum FROM backup_records WHERE path=?1 ORDER BY created_at DESC LIMIT 1",
            [backup_path],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .ok()
        .flatten()
    });
    database_value.or_else(|| {
        load_backup_manifest(root).ok().and_then(|entries| {
            entries
                .into_iter()
                .find(|entry| entry.path == backup_path)
                .map(|entry| entry.checksum)
        })
    })
}

fn verify_backup_checksum(
    root: &Path,
    backup_path: &str,
    bytes: &[u8],
    conn: Option<&Connection>,
) -> Result<String, String> {
    let checksum = hex::encode(Sha256::digest(bytes));
    if let Some(expected) = expected_backup_checksum(root, backup_path, conn) {
        if expected != checksum {
            return Err("备份校验和不匹配，已拒绝还原".into());
        }
    }
    Ok(checksum)
}

fn backup_created_at(root: &Path, backup_path: &str, conn: Option<&Connection>) -> Option<String> {
    let database_value = conn.and_then(|conn| {
        conn.query_row(
            "SELECT created_at FROM backup_records WHERE path=?1 ORDER BY created_at DESC LIMIT 1",
            [backup_path],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .ok()
        .flatten()
    });
    database_value.or_else(|| {
        load_backup_manifest(root).ok().and_then(|entries| {
            entries
                .into_iter()
                .find(|entry| entry.path == backup_path)
                .map(|entry| entry.created_at)
        })
    })
}

fn ensure_restore_drill_unlocked(root: &Path, master: Option<&[u8; 32]>) -> Result<(), String> {
    if protection_config_path(root).exists() && master.is_none() {
        return Err("当前保护账套尚未解锁，无法进行恢复演练".into());
    }
    Ok(())
}

#[tauri::command]
fn initialize(
    state: State<'_, AppState>,
    password: Option<String>,
) -> Result<BootstrapInfo, String> {
    let _task = state.tasks.begin("startup", "启动与完整性检查")?;
    ensure_dirs(&state.root)?;
    let _ = remove_file_if_exists(&ui_ready_path(&state.root));
    let mut guard = state.db.lock().map_err(to_error)?;
    let already_initialized = guard.is_some();
    if !already_initialized && password.is_some() {
        // A rejected retry must not leave a previously resolved key available
        // to recovery actions under a different credential.
        *state.protection_master.lock().map_err(to_error)? = None;
    }
    let (previous_exit, startup_master) = if already_initialized {
        (
            read_previous_exit(&state.root),
            *state.protection_master.lock().map_err(to_error)?,
        )
    } else {
        mark_startup_in_progress(&state.root, password.as_deref())?
    };
    if !already_initialized {
        // Keep the resolved master available even if opening or checking the
        // database fails; recovery must still be able to decode .db.enc files.
        *state.protection_master.lock().map_err(to_error)? = startup_master;
        decrypt_database_if_needed(&state.root, startup_master.as_ref())?;
        let path = state.root.join("data").join("erp.db");
        let conn = open_database(&path)?;
        if let Err(error) = migrate(&conn).and_then(|_| integrity_check(&conn)) {
            let _ = append_runtime_log(
                &state.root,
                "startup_integrity_failed",
                json!({"error": error}),
            );
            return Err(format!(
                "{error}。当前账套未打开，请从备份列表选择可用备份还原。"
            ));
        }
        *guard = Some(conn);
    }
    *state.previous_exit_dirty.lock().map_err(to_error)? = previous_exit.dirty;
    let protection_enabled: i64 = guard
        .as_ref()
        .and_then(|conn| {
            conn.query_row(
                "SELECT value FROM settings WHERE key='protection_enabled'",
                [],
                |r| r.get(0),
            )
            .ok()
        })
        .and_then(|v: String| v.parse().ok())
        .unwrap_or(0);
    let protection_enabled =
        protection_enabled == 1 || protection_config_path(&state.root).exists();
    let close_behavior = guard
        .as_ref()
        .map(close_behavior_from_connection)
        .unwrap_or_else(|| "minimize_to_tray".to_string());
    let previous_exit_time = previous_exit.time;
    let recovery_log_path = state.root.join("logs").to_string_lossy().to_string();
    if previous_exit.dirty {
        let _ = append_runtime_log(
            &state.root,
            "unclean_exit_recovered",
            json!({
                "previous_exit_time": previous_exit_time.clone(),
                "integrity": "verified",
            }),
        );
    }
    Ok(BootstrapInfo {
        app_version: APP_VERSION.to_string(),
        schema_version: SCHEMA_VERSION,
        root: state.root.to_string_lossy().to_string(),
        previous_exit_dirty: previous_exit.dirty,
        previous_exit_time,
        startup_integrity: "verified".to_string(),
        recovery_log_path,
        protection_enabled,
        close_behavior,
    })
}

#[tauri::command]
fn mark_ui_ready(state: State<'_, AppState>) -> Result<Value, String> {
    let payload = json!({
        "ready_at": now_iso(),
        "app_version": APP_VERSION,
    });
    write_bytes_atomically(
        &ui_ready_path(&state.root),
        &serde_json::to_vec_pretty(&payload).map_err(to_error)?,
    )?;
    Ok(payload)
}

#[tauri::command]
fn set_protection(state: State<'_, AppState>, password: String) -> Result<Value, String> {
    let _task = state.tasks.begin("protection", "迁移保护模式附件")?;
    if password.chars().count() < 8 {
        return Err("本地口令至少需要 8 个字符".into());
    }
    if protection_config_path(&state.root).exists() {
        return Err("保护模式已经配置，请使用修改或关闭流程".into());
    }
    let mut guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_mut()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    let salt = random_bytes::<16>();
    let master = random_bytes::<32>();
    let password_key = derive_password_key(&password, &salt)?;
    let recovery_key = hex::encode(random_bytes::<24>());
    let recovery_wrap_key = sha_key(&recovery_key);
    let config = ProtectionConfig {
        version: 1,
        salt_b64: B64.encode(salt),
        password_wrapped_b64: wrap_master(&password_key, &master)?,
        recovery_wrapped_b64: wrap_master(&recovery_wrap_key, &master)?,
    };
    save_protection_config(&state.root, &config)?;
    if let Err(error) = encrypt_attachments_for_enable(conn, &state.root, &master) {
        let _ = fs::remove_file(protection_config_path(&state.root));
        return Err(error);
    }
    if let Err(error) = conn
        .execute(
            "UPDATE settings SET value='1',updated_at=?1 WHERE key='protection_enabled'",
            [now_iso()],
        )
        .map_err(to_error)
    {
        // Keep the protection config because the attachment migration already committed.
        *state.protection_master.lock().map_err(to_error)? = Some(master);
        return Err(format!("附件已加密，但保护状态写入失败：{error}"));
    }
    *state.protection_master.lock().map_err(to_error)? = Some(master);
    Ok(
        json!({"enabled":true,"recovery_key":recovery_key,"warning":"请立即将恢复密钥保存到非 U 盘介质；关闭窗口后主库将加密落盘。"}),
    )
}

#[tauri::command]
fn disable_protection(state: State<'_, AppState>, secret: String) -> Result<Value, String> {
    let _task = state.tasks.begin("protection", "迁移未保护附件")?;
    let config =
        load_protection_config(&state.root)?.ok_or_else(|| "保护模式尚未配置".to_string())?;
    let master = resolve_master(&config, &secret)?;
    let mut guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_mut()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    decrypt_attachments_for_disable(conn, &state.root, &master)?;
    conn.execute(
        "UPDATE settings SET value='0',updated_at=?1 WHERE key='protection_enabled'",
        [now_iso()],
    )
    .map_err(to_error)?;
    let _ = fs::remove_file(protection_config_path(&state.root));
    let _ = fs::remove_file(encrypted_db_path(&state.root));
    *state.protection_master.lock().map_err(to_error)? = None;
    Ok(json!({"enabled":false}))
}

#[tauri::command]
fn list_entities(
    state: State<'_, AppState>,
    entity: String,
    search: Option<String>,
    status: Option<String>,
) -> Result<Vec<Value>, String> {
    let guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    let status_key = if entity == "statements" {
        Some("pay_status")
    } else if [
        "customers",
        "projects",
        "samples",
        "contracts",
        "orders",
        "purchases",
        "quotations",
    ]
    .contains(&entity.as_str())
    {
        Some(if ["customers", "projects"].contains(&entity.as_str()) {
            "stage"
        } else {
            "status"
        })
    } else {
        None
    };
    let rows = if entity == "attachments" {
        let master = *state.protection_master.lock().map_err(to_error)?;
        list_attachments_conn(conn, &state.root, master.as_ref(), None, None)?
    } else {
        list_entities_conn(conn, &entity)?
    };
    filter_records(rows, search.as_deref(), status.as_deref(), status_key).pipe(Ok)
}

#[tauri::command]
fn list_entities_page(
    state: State<'_, AppState>,
    entity: String,
    search: Option<String>,
    status: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
) -> Result<Value, String> {
    let guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    let status_key = if entity == "statements" {
        Some("pay_status")
    } else if [
        "customers",
        "projects",
        "samples",
        "contracts",
        "orders",
        "purchases",
        "quotations",
    ]
    .contains(&entity.as_str())
    {
        Some(if ["customers", "projects"].contains(&entity.as_str()) {
            "stage"
        } else {
            "status"
        })
    } else {
        None
    };
    if entity == "attachments" {
        let master = *state.protection_master.lock().map_err(to_error)?;
        let rows = list_attachments_conn(conn, &state.root, master.as_ref(), None, None)?;
        return paginate_records(
            filter_records(rows, search.as_deref(), status.as_deref(), status_key),
            page.unwrap_or(1),
            page_size.unwrap_or(20),
        );
    }
    let rows = list_entities_conn(conn, &entity)?;
    paginate_records(
        filter_records(rows, search.as_deref(), status.as_deref(), status_key),
        page.unwrap_or(1),
        page_size.unwrap_or(20),
    )
}

#[tauri::command]
fn list_attachments(
    state: State<'_, AppState>,
    object_type: Option<String>,
    object_id: Option<i64>,
) -> Result<Vec<Value>, String> {
    let _task = state.tasks.begin("attachment", "检查附件完整性")?;
    let normalized_type = object_type
        .map(|value| validate_attachment_object_type(&value))
        .transpose()?;
    if let Some(id) = object_id {
        if id <= 0 {
            return Err("对象 ID 必须为正数".into());
        }
    }
    let guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    let master = *state.protection_master.lock().map_err(to_error)?;
    list_attachments_conn(
        conn,
        &state.root,
        master.as_ref(),
        normalized_type.as_deref(),
        object_id,
    )
}

#[tauri::command]
fn save_attachment(
    state: State<'_, AppState>,
    object_type: String,
    object_id: i64,
    file_name: String,
    bytes: Vec<u8>,
) -> Result<Value, String> {
    let _task = state.tasks.begin("attachment", "保存附件")?;
    let mut guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_mut()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    let master = *state.protection_master.lock().map_err(to_error)?;
    save_attachment_to_connection(
        conn,
        &state.root,
        master.as_ref(),
        &object_type,
        object_id,
        &file_name,
        &bytes,
    )
}

#[tauri::command]
fn download_attachment(state: State<'_, AppState>, attachment_id: i64) -> Result<Value, String> {
    let _task = state.tasks.begin("attachment", "读取附件")?;
    let guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    let master = *state.protection_master.lock().map_err(to_error)?;
    read_attachment_from_connection(conn, &state.root, master.as_ref(), attachment_id)
}

fn normalize_setting(key: &str, value: &str) -> Result<String, String> {
    let key = key.trim();
    let value = value.trim();
    if key == "close_behavior" {
        return match value {
            "minimize_to_tray" | "tray" => Ok("minimize_to_tray".to_string()),
            "confirm_exit" | "exit" => Ok("confirm_exit".to_string()),
            _ => Err("关闭窗口行为只能选择最小化到托盘或询问后退出".into()),
        };
    }
    if key == "backup_schedule_enabled" || key == "cloud_sync_enabled" {
        return match value.to_ascii_lowercase().as_str() {
            "1" | "true" | "on" | "yes" => Ok("1".to_string()),
            "0" | "false" | "off" | "no" => Ok("0".to_string()),
            _ if key == "cloud_sync_enabled" => Err("自动同步只能设置为启用或停用".into()),
            _ => Err("定时自动备份只能设置为启用或停用".into()),
        };
    }
    let (minimum, maximum, label) = match key {
        "backup_retention" => (1_i64, 100_i64, "备份保留份数"),
        "reminder_days" => (0_i64, 365_i64, "回款临近提醒天数"),
        "backup_schedule_days" => (1_i64, 30_i64, "定时备份间隔天数"),
        _ => return Err(format!("不支持的设置项：{key}")),
    };
    let parsed = value
        .parse::<i64>()
        .map_err(|_| format!("{label}必须是整数"))?;
    if parsed < minimum || parsed > maximum {
        return Err(format!("{label}应在 {minimum} 至 {maximum} 之间"));
    }
    Ok(parsed.to_string())
}

fn close_behavior_from_connection(conn: &Connection) -> String {
    conn.query_row(
        "SELECT value FROM settings WHERE key='close_behavior'",
        [],
        |row| row.get::<_, String>(0),
    )
    .ok()
    .filter(|value| value == "confirm_exit" || value == "minimize_to_tray")
    .unwrap_or_else(|| "minimize_to_tray".to_string())
}

#[tauri::command]
fn list_settings(state: State<'_, AppState>) -> Result<Vec<Value>, String> {
    let guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    let mut stmt = conn
        .prepare("SELECT key,value,updated_at FROM settings ORDER BY key")
        .map_err(to_error)?;
    let rows = stmt.query_map([], |row| Ok(json!({"key": row.get::<_, String>(0)?, "value": row.get::<_, String>(1)?, "updated_at": row.get::<_, String>(2)?}))).map_err(to_error)?;
    rows.map(|row| row.map_err(to_error)).collect()
}

#[tauri::command]
fn save_settings(state: State<'_, AppState>, key: String, value: String) -> Result<Value, String> {
    let _task = state.tasks.begin("write", "保存设置")?;
    let normalized_key = key.trim().to_string();
    let normalized_value = normalize_setting(&normalized_key, &value)?;
    let mut guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_mut()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    let tx = conn.transaction().map_err(to_error)?;
    let before: Option<String> = tx
        .query_row(
            "SELECT value FROM settings WHERE key=?1",
            [&normalized_key],
            |row| row.get(0),
        )
        .optional()
        .map_err(to_error)?;
    let updated_at = now_iso();
    tx.execute(
        "INSERT INTO settings(key,value,updated_at) VALUES (?1,?2,?3) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",
        params![normalized_key, normalized_value, updated_at],
    ).map_err(to_error)?;
    audit(
        &tx,
        "settings",
        0,
        "update",
        before.map(|old| json!({"key":normalized_key,"value":old})),
        Some(json!({"key":normalized_key,"value":normalized_value})),
        None,
    )?;
    tx.commit().map_err(to_error)?;
    Ok(json!({"key": normalized_key, "value": normalized_value, "updated_at": updated_at}))
}

#[tauri::command]
fn get_sync_status(state: State<'_, AppState>) -> Result<Value, String> {
    let guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    let enabled = conn
        .query_row(
            "SELECT value FROM settings WHERE key='cloud_sync_enabled'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(to_error)?
        .is_some_and(|value| value == "1");
    let (pending_count, failed_count): (i64, i64) = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(CASE WHEN last_error <> '' THEN 1 ELSE 0 END),0) FROM sync_events WHERE synced_at IS NULL",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(to_error)?;
    let last_synced_at = conn
        .query_row(
            "SELECT MAX(synced_at) FROM sync_events WHERE synced_at IS NOT NULL",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .map_err(to_error)?;
    Ok(json!({
        "enabled": enabled,
        "pending_count": pending_count,
        "failed_count": failed_count,
        "last_synced_at": last_synced_at,
    }))
}

#[tauri::command]
fn list_pending_sync_events(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<Value>, String> {
    let guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    let limit = limit.unwrap_or(100).clamp(1, 500);
    let mut stmt = conn
        .prepare(
            "SELECT id,event_id,entity_type,entity_id,operation,payload_json,created_at,retry_count,last_error FROM sync_events WHERE synced_at IS NULL ORDER BY id LIMIT ?1",
        )
        .map_err(to_error)?;
    let rows = stmt
        .query_map([limit], |row| {
            let payload_text = row.get::<_, String>(5)?;
            let payload = serde_json::from_str::<Value>(&payload_text).unwrap_or(Value::Null);
            Ok(json!({
                "local_id": row.get::<_, i64>(0)?,
                "event_id": row.get::<_, String>(1)?,
                "entity_type": row.get::<_, String>(2)?,
                "entity_id": row.get::<_, i64>(3)?,
                "operation": row.get::<_, String>(4)?,
                "payload": payload,
                "created_at": row.get::<_, String>(6)?,
                "retry_count": row.get::<_, i64>(7)?,
                "last_error": row.get::<_, String>(8)?,
            }))
        })
        .map_err(to_error)?;
    rows.map(|row| row.map_err(to_error)).collect()
}

#[tauri::command]
fn mark_sync_events_synced(
    state: State<'_, AppState>,
    local_ids: Vec<i64>,
) -> Result<Value, String> {
    let _task = state.tasks.begin("write", "确认云端同步")?;
    let mut guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_mut()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    let tx = conn.transaction().map_err(to_error)?;
    let synced_at = now_iso();
    let mut updated = 0;
    for id in local_ids.into_iter().filter(|id| *id > 0).take(500) {
        updated += tx
            .execute(
                "UPDATE sync_events SET synced_at=?1,last_error='' WHERE id=?2 AND synced_at IS NULL",
                params![synced_at, id],
            )
            .map_err(to_error)?;
    }
    tx.commit().map_err(to_error)?;
    Ok(json!({"updated": updated, "synced_at": synced_at}))
}

#[tauri::command]
fn mark_sync_events_failed(
    state: State<'_, AppState>,
    local_ids: Vec<i64>,
    error: String,
) -> Result<Value, String> {
    let _task = state.tasks.begin("write", "记录同步失败")?;
    let message: String = error.trim().chars().take(500).collect();
    let mut guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_mut()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    let tx = conn.transaction().map_err(to_error)?;
    let mut updated = 0;
    for id in local_ids.into_iter().filter(|id| *id > 0).take(500) {
        updated += tx
            .execute(
                "UPDATE sync_events SET retry_count=retry_count+1,last_error=?1 WHERE id=?2 AND synced_at IS NULL",
                params![message, id],
            )
            .map_err(to_error)?;
    }
    tx.commit().map_err(to_error)?;
    Ok(json!({"updated": updated}))
}

fn dict_enabled(data: &Value) -> i64 {
    match data.get("enabled") {
        Some(Value::Bool(value)) => {
            if *value {
                1
            } else {
                0
            }
        }
        Some(Value::Number(value)) => {
            if value.as_i64().unwrap_or(0) != 0 {
                1
            } else {
                0
            }
        }
        Some(Value::String(value)) => {
            if matches!(
                value.trim().to_lowercase().as_str(),
                "1" | "true" | "yes" | "是"
            ) {
                1
            } else {
                0
            }
        }
        _ => 1,
    }
}

#[tauri::command]
fn list_dicts(state: State<'_, AppState>, dict_type: Option<String>) -> Result<Vec<Value>, String> {
    let wanted = dict_type.unwrap_or_default().trim().to_string();
    let guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id,type,value,label,sort_order,enabled FROM dicts ORDER BY type,sort_order,id",
        )
        .map_err(to_error)?;
    let rows = stmt.query_map([], |row| Ok(json!({"id":row.get::<_,i64>(0)?,"type":row.get::<_,String>(1)?,"value":row.get::<_,String>(2)?,"label":row.get::<_,String>(3)?,"sort_order":row.get::<_,i64>(4)?,"enabled":row.get::<_,i64>(5)? != 0}))).map_err(to_error)?;
    rows.filter_map(|row| match row {
        Ok(value)
            if wanted.is_empty()
                || value.get("type").and_then(Value::as_str) == Some(wanted.as_str()) =>
        {
            Some(Ok(value))
        }
        Ok(_) => None,
        Err(error) => Some(Err(to_error(error))),
    })
    .collect()
}

#[tauri::command]
fn save_dict(state: State<'_, AppState>, data: Value) -> Result<Value, String> {
    let _task = state.tasks.begin("write", "保存状态字典")?;
    let id = int_value(&data, "id");
    let dict_type = required_string(&data, "type", "字典类型")?;
    let value = required_string(&data, "value", "字典 code")?;
    let label = required_string(&data, "label", "字典显示名")?;
    if dict_type.len() > 64 || value.len() > 128 || label.len() > 128 {
        return Err("字典类型、code 或显示名过长".into());
    }
    let sort_order = int_value(&data, "sort_order").max(0);
    let enabled = dict_enabled(&data);
    let mut guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_mut()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    let tx = conn.transaction().map_err(to_error)?;
    let result = if id > 0 {
        let current: Option<(String, String, String, i64, i64)> = tx
            .query_row(
                "SELECT type,value,label,sort_order,enabled FROM dicts WHERE id=?1",
                [id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()
            .map_err(to_error)?;
        let (current_type, current_value, old_label, old_sort, old_enabled) =
            current.ok_or_else(|| "字典记录不存在".to_string())?;
        if current_type != dict_type || current_value != value {
            return Err("字典 code 不可修改；请停用旧项后新增新 code".into());
        }
        tx.execute(
            "UPDATE dicts SET label=?1,sort_order=?2,enabled=?3 WHERE id=?4",
            params![label, sort_order, enabled, id],
        )
        .map_err(|error| {
            if error.to_string().contains("UNIQUE") {
                "字典 code 已存在".to_string()
            } else {
                to_error(error)
            }
        })?;
        audit(
            &tx,
            "dicts",
            id,
            "update",
            Some(
                json!({"type":current_type,"value":current_value,"label":old_label,"sort_order":old_sort,"enabled":old_enabled != 0}),
            ),
            Some(
                json!({"type":dict_type,"value":value,"label":label,"sort_order":sort_order,"enabled":enabled != 0}),
            ),
            None,
        )?;
        id
    } else {
        tx.execute(
            "INSERT INTO dicts(type,value,label,sort_order,enabled) VALUES (?1,?2,?3,?4,?5)",
            params![dict_type, value, label, sort_order, enabled],
        )
        .map_err(|error| {
            if error.to_string().contains("UNIQUE") {
                "字典 code 已存在".to_string()
            } else {
                to_error(error)
            }
        })?;
        let new_id = tx.last_insert_rowid();
        audit(
            &tx,
            "dicts",
            new_id,
            "create",
            None,
            Some(
                json!({"type":dict_type,"value":value,"label":label,"sort_order":sort_order,"enabled":enabled != 0}),
            ),
            None,
        )?;
        new_id
    };
    tx.commit().map_err(to_error)?;
    Ok(
        json!({"id":result,"type":dict_type,"value":value,"label":label,"sort_order":sort_order,"enabled":enabled != 0}),
    )
}

fn quotation_base_no(no: &str) -> String {
    let trimmed = no.trim();
    if let Some(index) = trimmed.rfind("-V") {
        let suffix = &trimmed[index + 2..];
        if !suffix.is_empty() && suffix.chars().all(|value| value.is_ascii_digit()) {
            return trimmed[..index].to_string();
        }
    }
    trimmed.to_string()
}

fn next_quotation_version_no(
    tx: &Transaction<'_>,
    source_no: &str,
    source_version: i64,
) -> Result<(String, i64), String> {
    let base = quotation_base_no(source_no);
    let mut version = source_version.max(1) + 1;
    loop {
        let candidate = format!("{base}-V{version}");
        let exists: i64 = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM quotations WHERE no=?1)",
                [&candidate],
                |row| row.get(0),
            )
            .map_err(to_error)?;
        if exists == 0 {
            return Ok((candidate, version));
        }
        version += 1;
    }
}

fn next_order_no(tx: &Transaction<'_>, requested: &str) -> Result<String, String> {
    let requested = requested.trim();
    if !requested.is_empty() {
        let exists: i64 = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM orders WHERE no=?1)",
                [requested],
                |row| row.get(0),
            )
            .map_err(to_error)?;
        if exists != 0 {
            return Err("订单号已存在，请更换订单号".into());
        }
        return Ok(requested.to_string());
    }
    let prefix = format!("SO-{}", Local::now().format("%Y%m%d"));
    let mut sequence = 1_i64;
    loop {
        let candidate = format!("{prefix}-{sequence:03}");
        let exists: i64 = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM orders WHERE no=?1)",
                [&candidate],
                |row| row.get(0),
            )
            .map_err(to_error)?;
        if exists == 0 {
            return Ok(candidate);
        }
        sequence += 1;
    }
}

fn duplicate_quotation_tx(tx: &Transaction<'_>, quotation_id: i64) -> Result<i64, String> {
    let source = current_record(tx, "quotations", quotation_id)?;
    let source_no = required_string(&source, "no", "报价单号")?;
    let source_version = int_value(&source, "version").max(1);
    let (new_no, new_version) = next_quotation_version_no(tx, &source_no, source_version)?;
    let items = list_document_items_conn(tx, "quotations", quotation_id)?;
    let mut data = source;
    let audit_no = new_no.clone();
    {
        let object = data
            .as_object_mut()
            .ok_or_else(|| "报价记录格式无效".to_string())?;
        object.insert("id".into(), json!(0));
        object.insert("no".into(), json!(new_no));
        object.insert("version".into(), json!(new_version));
        object.insert("status".into(), json!("draft"));
        object.insert("quote_date".into(), json!(today()));
        object.insert("valid_until".into(), Value::Null);
        object.insert("items".into(), Value::Array(items));
    }
    let new_id = save_record_tx(tx, "quotations", &data)?;
    audit(
        tx,
        "quotations",
        new_id,
        "duplicate",
        Some(json!({"source_id": quotation_id})),
        Some(json!({"version": new_version, "no": audit_no})),
        None,
    )?;
    Ok(new_id)
}

fn quotation_item_signature(item: &Value) -> Value {
    json!({
        "material_id": item.get("material_id").cloned().unwrap_or(Value::Null),
        "batch": item.get("batch").cloned().unwrap_or(Value::Null),
        "material_category": item.get("material_category").cloned().unwrap_or(Value::Null),
        "material_grade": item.get("material_grade").cloned().unwrap_or(Value::Null),
        "manufacturer": item.get("manufacturer").cloned().unwrap_or(Value::Null),
        "note": item.get("note").cloned().unwrap_or(Value::Null),
        "qty_grams": item.get("qty_grams").cloned().unwrap_or(Value::Null),
        "unit_price_cents": item.get("unit_price_cents").cloned().unwrap_or(Value::Null),
        "amount_cents": item.get("amount_cents").cloned().unwrap_or(Value::Null),
    })
}

fn compare_quotations_conn(
    conn: &Connection,
    left_id: i64,
    right_id: i64,
) -> Result<Value, String> {
    if left_id <= 0 || right_id <= 0 || left_id == right_id {
        return Err("请选择两条不同的报价版本".into());
    }
    let left = current_record(conn, "quotations", left_id)?;
    let right = current_record(conn, "quotations", right_id)?;
    let fields = [
        ("customer_id", "客户"),
        ("recipient_name", "收件单位"),
        ("sender_name", "发件人"),
        ("recipient_contact", "收件人"),
        ("recipient_fax", "传真号"),
        ("cc", "抄送"),
        ("page_count", "页数"),
        ("request_review", "请审阅"),
        ("request_comment", "请批注"),
        ("quote_date", "报价日期"),
        ("subject", "主题"),
        ("price_note", "价格说明"),
        ("adjustment_note", "调价说明"),
        ("footer_address", "页脚地址"),
        ("footer_phone", "页脚电话"),
        ("footer_fax", "页脚传真"),
        ("footer_email", "页脚邮箱"),
    ];
    let mut header_differences = Vec::new();
    for (field, label) in fields {
        let left_value = left.get(field).cloned().unwrap_or(Value::Null);
        let right_value = right.get(field).cloned().unwrap_or(Value::Null);
        if left_value != right_value {
            header_differences.push(json!({
                "field": field,
                "label": label,
                "left": left_value,
                "right": right_value,
            }));
        }
    }
    let left_items = list_document_items_conn(conn, "quotations", left_id)?;
    let right_items = list_document_items_conn(conn, "quotations", right_id)?;
    let mut item_differences = Vec::new();
    for index in 0..left_items.len().max(right_items.len()) {
        let left_item = left_items.get(index);
        let right_item = right_items.get(index);
        let left_signature = left_item.map(quotation_item_signature);
        let right_signature = right_item.map(quotation_item_signature);
        if left_signature != right_signature {
            let change = match (left_item, right_item) {
                (None, Some(_)) => "added",
                (Some(_), None) => "removed",
                _ => "changed",
            };
            item_differences.push(json!({
                "index": index + 1,
                "change": change,
                "left": left_signature.unwrap_or(Value::Null),
                "right": right_signature.unwrap_or(Value::Null),
            }));
        }
    }
    Ok(json!({
        "left": {"id": left_id, "no": left.get("no"), "version": left.get("version")},
        "right": {"id": right_id, "no": right.get("no"), "version": right.get("version")},
        "header_differences": header_differences,
        "item_differences": item_differences,
        "same": header_differences.is_empty() && item_differences.is_empty(),
    }))
}

fn convert_quotation_to_order_tx(
    tx: &Transaction<'_>,
    quotation_id: i64,
    requested_order_no: &str,
    delivery_date: Option<&str>,
) -> Result<i64, String> {
    let quotation = current_record(tx, "quotations", quotation_id)?;
    let status = str_value(&quotation, "status");
    if matches!(status.as_str(), "expired" | "rejected") {
        return Err("已过期或已拒绝的报价不能转为订单".into());
    }
    let mut items = list_document_items_conn(tx, "quotations", quotation_id)?;
    if items.is_empty() {
        items.push(json!({
            "material_id": int_value(&quotation, "material_id"),
            "qty_grams": int_value(&quotation, "moq_grams"),
            "unit_price_cents": int_value(&quotation, "price_cents"),
            "batch": "",
        }));
    }
    let first = items
        .first()
        .ok_or_else(|| "报价没有可转订单的明细".to_string())?;
    let customer_id = int_value(&quotation, "customer_id");
    let first_material_id = int_value(first, "material_id");
    let first_price = int_value(first, "unit_price_cents");
    if customer_id <= 0 || first_material_id <= 0 || first_price <= 0 {
        return Err("报价客户、牌号和单价信息不完整，不能转为订单".into());
    }
    let total_qty: i64 = items.iter().map(|item| int_value(item, "qty_grams")).sum();
    if total_qty <= 0 {
        return Err("报价数量必须大于 0，不能转为订单".into());
    }
    let order_no = next_order_no(tx, requested_order_no)?;
    let delivery_date = delivery_date
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    if let Some(value) = delivery_date.as_deref() {
        NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map_err(|_| "交期必须使用 YYYY-MM-DD 日期".to_string())?;
    }
    let order_data = json!({
        "id": 0,
        "no": order_no,
        "customer_id": customer_id,
        "material_id": first_material_id,
        "qty_grams": total_qty,
        "price_cents": first_price,
        "delivery_date": delivery_date,
        "status": "pending_confirm",
        "sign_status": "pending",
        "quotation_id": quotation_id,
        "items": items,
    });
    let order_id = save_record_tx(tx, "orders", &order_data)?;
    audit(
        tx,
        "orders",
        order_id,
        "create_from_quotation",
        Some(json!({"quotation_id": quotation_id})),
        Some(json!({"order_id": order_id})),
        None,
    )?;
    Ok(order_id)
}

#[tauri::command]
fn duplicate_quotation(state: State<'_, AppState>, quotation_id: i64) -> Result<Value, String> {
    let _task = state.tasks.begin("write", "复制报价版本")?;
    let mut guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_mut()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    let tx = conn.transaction().map_err(to_error)?;
    let new_id = duplicate_quotation_tx(&tx, quotation_id)?;
    tx.commit().map_err(to_error)?;
    current_record(conn, "quotations", new_id)
}

#[tauri::command]
fn compare_quotations(
    state: State<'_, AppState>,
    left_id: i64,
    right_id: i64,
) -> Result<Value, String> {
    let _task = state.tasks.begin("read", "比较报价版本")?;
    let guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    compare_quotations_conn(conn, left_id, right_id)
}

#[tauri::command]
fn convert_quotation_to_order(
    state: State<'_, AppState>,
    quotation_id: i64,
    order_no: String,
    delivery_date: Option<String>,
) -> Result<Value, String> {
    let _task = state.tasks.begin("write", "报价转订单")?;
    let mut guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_mut()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    let tx = conn.transaction().map_err(to_error)?;
    let order_id =
        convert_quotation_to_order_tx(&tx, quotation_id, &order_no, delivery_date.as_deref())?;
    tx.commit().map_err(to_error)?;
    current_record(conn, "orders", order_id)
}

#[tauri::command]
fn save_entity(state: State<'_, AppState>, entity: String, data: Value) -> Result<Value, String> {
    let _task = state.tasks.begin("write", "保存业务记录")?;
    let mut guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_mut()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    let before = int_value(&data, "id").checked_add(0).and_then(|id| {
        if id > 0 {
            current_record(conn, &entity, id).ok()
        } else {
            None
        }
    });
    let tx = conn.transaction().map_err(to_error)?;
    let id = save_record_tx(&tx, &entity, &data)?;
    let after = current_record(&tx, &entity, id)?;
    audit(
        &tx,
        &entity,
        id,
        if before.is_some() { "update" } else { "create" },
        before,
        Some(after.clone()),
        None,
    )?;
    tx.commit().map_err(to_error)?;
    current_record(conn, &entity, id)
}

#[tauri::command]
fn list_document_items(
    state: State<'_, AppState>,
    document_type: String,
    document_id: i64,
) -> Result<Vec<Value>, String> {
    let guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    list_document_items_conn(conn, &document_type, document_id)
}

#[tauri::command]
fn save_document_items(
    state: State<'_, AppState>,
    document_type: String,
    document_id: i64,
    items: Vec<Value>,
) -> Result<Value, String> {
    let _task = state.tasks.begin("write", "保存单据明细")?;
    let mut guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_mut()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    let before = list_document_items_conn(conn, &document_type, document_id).ok();
    let tx = conn.transaction().map_err(to_error)?;
    let total = save_document_items_tx(&tx, &document_type, document_id, &items)?;
    audit(
        &tx,
        &format!("{document_type}_items"),
        document_id,
        "replace",
        before.map(Value::Array),
        Some(json!({"count": items.len(), "total_cents": total})),
        None,
    )?;
    tx.commit().map_err(to_error)?;
    Ok(
        json!({"document_id": document_id, "total_cents": total, "items": list_document_items_conn(conn, &document_type, document_id)?}),
    )
}

#[tauri::command]
fn save_payment_allocations(
    state: State<'_, AppState>,
    payment_id: i64,
    allocations: Vec<Value>,
) -> Result<Value, String> {
    let _task = state.tasks.begin("write", "保存回款分配")?;
    let mut guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_mut()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    let before = list_document_items_conn(conn, "payments", payment_id)?;
    let tx = conn.transaction().map_err(to_error)?;
    let statement_ids = replace_payment_allocations(&tx, payment_id, &allocations)?;
    let first_statement = allocations
        .first()
        .map(|value| int_value(value, "statement_id"))
        .unwrap_or(0);
    if first_statement <= 0 {
        return Err("至少需要一条有效回款分配".into());
    }
    tx.execute(
        "UPDATE payments SET statement_id=?1 WHERE id=?2",
        params![first_statement, payment_id],
    )
    .map_err(to_error)?;
    for statement_id in &statement_ids {
        refresh_statement(&tx, *statement_id)?;
    }
    audit(
        &tx,
        "payment_allocations",
        payment_id,
        "replace",
        Some(json!(before)),
        Some(json!({"count": allocations.len()})),
        None,
    )?;
    tx.commit().map_err(to_error)?;
    Ok(
        json!({"payment_id": payment_id, "allocations": list_document_items_conn(conn, "payments", payment_id)?}),
    )
}

#[derive(Clone, Copy)]
struct CsvImportSpec {
    allowed: &'static [&'static str],
    required: &'static [&'static str],
}

fn csv_import_spec(entity: &str) -> Option<CsvImportSpec> {
    match entity {
        "customers" => Some(CsvImportSpec {
            allowed: &[
                "id",
                "name",
                "main_host",
                "direction",
                "material_system",
                "stage",
                "next_follow_date",
                "notes",
            ],
            required: &["name"],
        }),
        "materials" => Some(CsvImportSpec {
            allowed: &[
                "id",
                "code",
                "base_resin",
                "modification",
                "mi",
                "impact",
                "hdt",
                "density",
                "supplier_id",
                "cost_cents",
                "cost_date",
            ],
            required: &["code"],
        }),
        "quotations" => Some(CsvImportSpec {
            allowed: &[
                "id",
                "no",
                "customer_id",
                "material_id",
                "price_cents",
                "moq_grams",
                "freight",
                "valid_until",
                "version",
                "status",
            ],
            required: &["no", "customer_id", "material_id", "price_cents"],
        }),
        "orders" => Some(CsvImportSpec {
            allowed: &[
                "id",
                "no",
                "customer_id",
                "material_id",
                "qty_grams",
                "price_cents",
                "delivery_date",
                "status",
                "sign_status",
                "quotation_id",
            ],
            required: &[
                "no",
                "customer_id",
                "material_id",
                "qty_grams",
                "price_cents",
            ],
        }),
        "statements" => Some(CsvImportSpec {
            allowed: &[
                "id",
                "no",
                "customer_id",
                "period_start",
                "period_end",
                "account_days",
                "promise_date",
            ],
            required: &["no", "customer_id", "period_start", "period_end"],
        }),
        "purchases" => Some(CsvImportSpec {
            allowed: &[
                "id",
                "no",
                "supplier_id",
                "material_id",
                "qty_grams",
                "status",
                "eta",
            ],
            required: &["no", "supplier_id", "material_id", "qty_grams"],
        }),
        _ => None,
    }
}

fn import_numeric_field(key: &str) -> bool {
    matches!(
        key,
        "id" | "customer_id"
            | "material_id"
            | "supplier_id"
            | "quotation_id"
            | "qty_grams"
            | "weight_grams"
            | "price_cents"
            | "cost_cents"
            | "amount_cents"
            | "moq_grams"
            | "account_days"
            | "version"
    )
}

fn import_date_field(key: &str) -> bool {
    matches!(
        key,
        "next_follow_date"
            | "cost_date"
            | "valid_until"
            | "delivery_date"
            | "period_start"
            | "period_end"
            | "promise_date"
            | "eta"
    )
}

fn normalize_import_header(value: &str, first: bool) -> String {
    let trimmed = value.trim();
    if first {
        trimmed.trim_start_matches('\u{feff}').trim().to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_import_value(key: &str, raw: &str) -> Result<Value, String> {
    let value = raw.trim();
    if value.is_empty() {
        return Ok(Value::String(String::new()));
    }
    if import_numeric_field(key) {
        let parsed = value
            .parse::<i64>()
            .map_err(|_| format!("字段 {key} 必须是整数"))?;
        return Ok(json!(parsed));
    }
    if import_date_field(key) && NaiveDate::parse_from_str(value, "%Y-%m-%d").is_err() {
        return Err(format!("字段 {key} 必须使用 YYYY-MM-DD 日期"));
    }
    Ok(Value::String(value.to_string()))
}

#[derive(Debug)]
struct ParsedImportRow {
    line: usize,
    values: Vec<String>,
    data: Option<Value>,
    error: Option<String>,
}

#[derive(Debug)]
struct ParsedImport {
    headers: Vec<String>,
    rows: Vec<ParsedImportRow>,
}

fn parse_import_csv(entity: &str, bytes: &[u8]) -> Result<ParsedImport, String> {
    let spec = csv_import_spec(entity).ok_or_else(|| "当前模块不支持 CSV 导入".to_string())?;
    let mut reader = csv::ReaderBuilder::new().flexible(true).from_reader(bytes);
    let raw_headers = reader
        .headers()
        .map_err(|error| format!("CSV 首行读取失败：{error}"))?
        .clone();
    let headers: Vec<String> = raw_headers
        .iter()
        .enumerate()
        .map(|(index, value)| normalize_import_header(value, index == 0))
        .collect();
    if headers.is_empty() || headers.iter().all(|value| value.is_empty()) {
        return Err("CSV 首行必须包含字段名".into());
    }
    let mut header_errors = Vec::new();
    let mut seen = HashSet::new();
    for header in &headers {
        if header.is_empty() {
            header_errors.push("字段名不能为空".to_string());
        } else if !spec.allowed.contains(&header.as_str()) {
            header_errors.push(format!("不支持的字段：{header}"));
        } else if !seen.insert(header.clone()) {
            header_errors.push(format!("字段重复：{header}"));
        }
    }
    for required in spec.required {
        if !seen.contains(*required) {
            header_errors.push(format!("缺少必填字段：{required}"));
        }
    }
    if !header_errors.is_empty() {
        return Err(header_errors.join("；"));
    }

    let mut rows = Vec::new();
    for (offset, item) in reader.records().enumerate() {
        let line = offset + 2;
        let record = item.map_err(|error| format!("第 {line} 行 CSV 解析失败：{error}"))?;
        let values: Vec<String> = record.iter().map(ToString::to_string).collect();
        if values.iter().all(|value| value.trim().is_empty()) {
            continue;
        }
        if values.len() != headers.len() {
            rows.push(ParsedImportRow {
                line,
                values,
                data: None,
                error: Some(format!(
                    "字段数量为 {}，应为 {}",
                    record.len(),
                    headers.len()
                )),
            });
            continue;
        }
        let mut object = serde_json::Map::new();
        let mut row_error = None;
        for (index, key) in headers.iter().enumerate() {
            match normalize_import_value(key, record.get(index).unwrap_or_default()) {
                Ok(value) => {
                    object.insert(key.clone(), value);
                }
                Err(error) => {
                    row_error = Some(error);
                    break;
                }
            }
        }
        rows.push(ParsedImportRow {
            line,
            values,
            data: row_error.is_none().then_some(Value::Object(object)),
            error: row_error,
        });
    }
    if rows.is_empty() {
        return Err("CSV 中没有可导入的数据行".into());
    }
    Ok(ParsedImport { headers, rows })
}

fn save_import_row(tx: &Transaction<'_>, entity: &str, data: &Value) -> Result<i64, String> {
    tx.execute_batch("SAVEPOINT erp_import_row")
        .map_err(to_error)?;
    match save_record_tx(tx, entity, data) {
        Ok(id) => {
            if let Err(error) = tx.execute_batch("RELEASE SAVEPOINT erp_import_row") {
                let _ = tx.execute_batch(
                    "ROLLBACK TO SAVEPOINT erp_import_row; RELEASE SAVEPOINT erp_import_row",
                );
                return Err(to_error(error));
            }
            Ok(id)
        }
        Err(error) => {
            tx.execute_batch(
                "ROLLBACK TO SAVEPOINT erp_import_row; RELEASE SAVEPOINT erp_import_row",
            )
            .map_err(|cleanup| format!("{error}（行级回滚失败：{}）", to_error(cleanup)))?;
            Err(error)
        }
    }
}

fn is_duplicate_import_error(error: &str) -> bool {
    error.contains("UNIQUE constraint failed") || error.contains("唯一") || error.contains("已存在")
}

#[cfg(test)]
fn write_failed_import_file(
    root: &Path,
    batch_id: &str,
    headers: &[String],
    failures: &[(usize, Vec<String>, String)],
) -> Result<Option<String>, String> {
    write_failed_import_file_with_master(root, batch_id, headers, failures, None)
}

fn write_failed_import_file_with_master(
    root: &Path,
    batch_id: &str,
    headers: &[String],
    failures: &[(usize, Vec<String>, String)],
    master: Option<&[u8; 32]>,
) -> Result<Option<String>, String> {
    if failures.is_empty() {
        return Ok(None);
    }
    let mut writer = csv::WriterBuilder::new()
        .has_headers(false)
        .from_writer(Vec::<u8>::new());
    let mut output_headers = headers.to_vec();
    output_headers.push("line".to_string());
    output_headers.push("error".to_string());
    writer
        .write_record(output_headers.iter())
        .map_err(|error| format!("CSV 内容生成失败：{error}"))?;
    for (line, values, error) in failures {
        let mut output = values.clone();
        while output.len() < headers.len() {
            output.push(String::new());
        }
        output.push(line.to_string());
        output.push(error.clone());
        writer
            .write_record(output.iter())
            .map_err(|error| format!("CSV 内容生成失败：{error}"))?;
    }
    writer
        .flush()
        .map_err(|error| format!("CSV 内容生成失败：{error}"))?;
    let bytes = writer
        .into_inner()
        .map_err(|error| format!("CSV 内容生成失败：{}", error.error()))?;
    let (path, _) = write_csv_payload(root, &format!("import-failed-{batch_id}"), &bytes, master)?;
    Ok(Some(path))
}

fn record_import_batch(
    conn: &Connection,
    batch_id: &str,
    entity: &str,
    file_name: &str,
    checksum: &str,
    inserted: i64,
    skipped: i64,
    failed: i64,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO import_batches(batch_id,entity,source_file,source_checksum,inserted_count,skipped_count,failed_count,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![batch_id, entity, file_name, checksum, inserted, skipped, failed, now_iso()],
    ).map_err(to_error)?;
    Ok(())
}

fn record_import_batch_tx(
    tx: &Transaction<'_>,
    batch_id: &str,
    entity: &str,
    file_name: &str,
    checksum: &str,
    inserted: i64,
    skipped: i64,
    failed: i64,
) -> Result<(), String> {
    tx.execute(
        "INSERT INTO import_batches(batch_id,entity,source_file,source_checksum,inserted_count,skipped_count,failed_count,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![batch_id, entity, file_name, checksum, inserted, skipped, failed, now_iso()],
    ).map_err(to_error)?;
    Ok(())
}

#[tauri::command]
fn download_template(state: State<'_, AppState>, entity: String) -> Result<Value, String> {
    let _task = state.tasks.begin("export", "生成导入模板")?;
    ensure_dirs(&state.root)?;
    let (headers, sample, note) = match entity.as_str() {
        "customers" => (
            vec![
                "name",
                "main_host",
                "direction",
                "material_system",
                "stage",
                "next_follow_date",
                "notes",
            ],
            vec![
                "示例客户",
                "示例主机厂",
                "内饰",
                "PC/ABS",
                "lead",
                "2026-10-01",
                "删除示例行后填写",
            ],
            "stage 使用 customer_stage 字典 code。",
        ),
        "materials" => (
            vec![
                "code",
                "base_resin",
                "modification",
                "mi",
                "impact",
                "hdt",
                "density",
                "supplier_id",
                "cost_cents",
                "cost_date",
            ],
            vec![
                "PP-TD20",
                "PP",
                "TD",
                "",
                "",
                "",
                "1.04",
                "",
                "920",
                "2026-10-01",
            ],
            "金额字段使用分；supplier_id 可留空。",
        ),
        "quotations" => (
            vec![
                "no",
                "customer_id",
                "material_id",
                "price_cents",
                "moq_grams",
                "freight",
                "valid_until",
                "version",
                "status",
            ],
            vec![
                "Q-EXAMPLE-001",
                "1",
                "1",
                "2180",
                "50000",
                "卖方",
                "2026-10-31",
                "1",
                "draft",
            ],
            "先导入客户和材料，再导入报价。",
        ),
        "orders" => (
            vec![
                "no",
                "customer_id",
                "material_id",
                "qty_grams",
                "price_cents",
                "delivery_date",
                "status",
                "sign_status",
                "quotation_id",
            ],
            vec![
                "SO-EXAMPLE-001",
                "1",
                "1",
                "100000",
                "2180",
                "2026-10-31",
                "pending_confirm",
                "pending",
                "",
            ],
            "数量使用克，金额使用分。",
        ),
        "statements" => (
            vec![
                "no",
                "customer_id",
                "period_start",
                "period_end",
                "account_days",
                "promise_date",
            ],
            vec![
                "AR-EXAMPLE-001",
                "1",
                "2026-10-01",
                "2026-10-31",
                "45",
                "2026-11-30",
            ],
            "对账明细可在系统内按订单分配。",
        ),
        "purchases" => (
            vec![
                "no",
                "supplier_id",
                "material_id",
                "qty_grams",
                "status",
                "eta",
            ],
            vec![
                "PO-EXAMPLE-001",
                "1",
                "1",
                "100000",
                "pending_quote",
                "2026-10-20",
            ],
            "先导入供应商和材料。",
        ),
        _ => return Err("当前仅提供客户、材料、报价、订单、对账和采购模板".into()),
    };
    let stamp = Local::now().format("%Y%m%d-%H%M%S%.3f").to_string();
    let path = state
        .root
        .join("export")
        .join(format!("template-{entity}-{stamp}.csv"));
    let note_path = state
        .root
        .join("export")
        .join(format!("template-{entity}-{stamp}-说明.txt"));
    let mut writer = csv::WriterBuilder::new()
        .has_headers(false)
        .from_path(&path)
        .map_err(|error| csv_file_error(&path, error))?;
    writer.write_record(headers.iter()).map_err(to_error)?;
    writer.write_record(sample.iter()).map_err(to_error)?;
    writer
        .flush()
        .map_err(|error| file_operation_error(&path, error))?;
    fs::write(&note_path, format!("{entity} UTF-8 CSV 模板\r\n{note}\r\n首行是字段名，第二行是示例数据；导入前请删除示例行并按字段字典填写。\r\n")).map_err(|error| file_operation_error(&note_path, error))?;
    Ok(
        json!({"path": path.to_string_lossy().to_string(), "note_path": note_path.to_string_lossy().to_string()}),
    )
}

#[tauri::command]
fn delete_entity(state: State<'_, AppState>, entity: String, id: i64) -> Result<Value, String> {
    let _task = state.tasks.begin("write", "归档业务记录")?;
    let mut guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_mut()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    let before = current_record(conn, &entity, id)?;
    let table = match entity.as_str() {
        "customers" | "contacts" | "projects" | "materials" | "samples" | "contracts"
        | "quotations" | "orders" | "deliveries" | "statements" | "purchases" => entity.as_str(),
        "suppliers" => "suppliers",
        _ => return Err("该记录只能通过业务流程处理，不能直接归档".into()),
    };
    let tx = conn.transaction().map_err(to_error)?;
    tx.execute(
        &format!("UPDATE {table} SET archived_at=?1,updated_at=?2 WHERE id=?3"),
        params![now_iso(), now_iso(), id],
    )
    .map_err(to_error)?;
    audit(
        &tx,
        &entity,
        id,
        "archive",
        Some(before),
        None,
        Some("用户归档"),
    )?;
    tx.commit().map_err(to_error)?;
    Ok(json!({"ok": true}))
}

fn transition_label(tx: &Transaction<'_>, dict_type: &str, value: &str) -> Result<String, String> {
    let label = tx
        .query_row(
            "SELECT label FROM dicts WHERE type=?1 AND value=?2",
            params![dict_type, value],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(to_error)?;
    Ok(label
        .filter(|label| !label.trim().is_empty())
        .unwrap_or_else(|| value.to_string()))
}

fn transition_entity_conn(
    conn: &mut Connection,
    entity: &str,
    id: i64,
    next_status: &str,
    reason: Option<&str>,
) -> Result<Value, String> {
    let field = transition_field(entity).ok_or_else(|| "该对象没有可流转状态".to_string())?;
    let before = current_record(conn, entity, id)?;
    let current = before
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if !allowed_transition(entity, &current, next_status) {
        return Err(format!("不允许从“{}”流转到“{}”", current, next_status));
    }
    let normalized_reason = reason.map(str::trim).filter(|value| !value.is_empty());
    if needs_reason(&current, next_status) && normalized_reason.is_none() {
        return Err("该状态变更必须填写原因".into());
    }
    if entity == "statements" {
        return Err("对账状态由应收余额、承诺回款日和提醒参数自动计算，不能手工流转".into());
    }
    let table = match entity {
        "customers" => "customers",
        "projects" => "projects",
        "quotations" => "quotations",
        "contracts" => "contracts",
        "samples" => "samples",
        "orders" => "orders",
        "purchases" => "purchases",
        "statements" => "statements",
        _ => return Err("不支持的状态对象".into()),
    };
    let next_follow_date = before
        .get("next_follow_date")
        .and_then(Value::as_str)
        .map(str::to_string);
    let tx = conn.transaction().map_err(to_error)?;
    if entity == "samples" && matches!(next_status, "failed" | "retest") {
        tx.execute(
            "UPDATE samples SET fail_reason=?1 WHERE id=?2",
            params![normalized_reason.unwrap_or_default(), id],
        )
        .map_err(to_error)?;
    }
    validate_status_entry(&tx, entity, id, next_status)?;
    tx.execute(
        &format!("UPDATE {table} SET {field}=?1,updated_at=?2 WHERE id=?3"),
        params![next_status, now_iso(), id],
    )
    .map_err(to_error)?;
    if entity == "orders" {
        if next_status == "signed" {
            tx.execute("UPDATE orders SET sign_status='signed' WHERE id=?1", [id])
                .map_err(to_error)?;
        }
        refresh_order_reservation(&tx, id)?;
    }
    if entity == "purchases" {
        let material_id: i64 = tx
            .query_row("SELECT material_id FROM purchases WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .map_err(to_error)?;
        if next_status == "received" {
            record_purchase_receipt(&tx, id)?;
        }
        refresh_inventory_transit(&tx, material_id)?;
    }
    if entity == "customers" && current != next_status {
        let current_label = transition_label(&tx, "customer_stage", &current)?;
        let next_label = transition_label(&tx, "customer_stage", next_status)?;
        let mut content = format!("客户阶段由“{}”流转至“{}”。", current_label, next_label);
        if let Some(reason) = normalized_reason {
            content.push_str(&format!(" 原因：{reason}"));
        }
        tx.execute(
            "INSERT INTO follow_ups(customer_id,content,follow_date,next_date,created_at) VALUES (?1,?2,?3,?4,?5)",
            params![id, content, today(), next_follow_date, now_iso()],
        )
        .map_err(to_error)?;
    }
    let after = json!({"id": id, field: next_status});
    audit(
        &tx,
        entity,
        id,
        "transition",
        Some(before),
        Some(after),
        normalized_reason,
    )?;
    tx.commit().map_err(to_error)?;
    current_record(conn, entity, id)
}

#[tauri::command]
fn transition_entity(
    state: State<'_, AppState>,
    entity: String,
    id: i64,
    next_status: String,
    reason: Option<String>,
) -> Result<Value, String> {
    let _task = state.tasks.begin("write", "更新业务状态")?;
    let mut guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_mut()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    transition_entity_conn(conn, &entity, id, &next_status, reason.as_deref())
}

fn dashboard_data(conn: &Connection, current_day: &str) -> Result<Value, String> {
    let customers = list_entities_conn(conn, "customers")?;
    let follow_ups = list_entities_conn(conn, "follow_ups")?;
    let orders = list_entities_conn(conn, "orders")?;
    let statements = list_entities_conn(conn, "statements")?;
    let payments = list_entities_conn(conn, "payments")?;
    let month = format!("{}-{:02}", Local::now().year(), Local::now().month());
    let monthly_sales: i64 = orders
        .iter()
        .filter(|row| {
            row.get("delivery_date")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .starts_with(&month)
        })
        .map(|row| row.get("amount_cents").and_then(Value::as_i64).unwrap_or(0))
        .sum();
    let monthly_received: i64 = payments
        .iter()
        .filter(|row| {
            row.get("pay_date")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .starts_with(&month)
        })
        .map(|row| row.get("amount_cents").and_then(Value::as_i64).unwrap_or(0))
        .sum();
    let unpaid: i64 = statements
        .iter()
        .map(|row| row.get("unpaid_cents").and_then(Value::as_i64).unwrap_or(0))
        .sum();
    let mut seen_follow_up_customers = HashSet::new();
    let latest_follow_ups: Vec<&Value> = follow_ups
        .iter()
        .filter(|row| {
            row.get("customer_id")
                .and_then(Value::as_i64)
                .map(|customer_id| seen_follow_up_customers.insert(customer_id))
                .unwrap_or(false)
        })
        .collect();
    let overdue_count = latest_follow_ups
        .iter()
        .filter(|row| {
            row.get("next_date")
                .and_then(Value::as_str)
                .map(|date| date < current_day)
                .unwrap_or(false)
        })
        .count();
    let stage_info = [
        ("lead", "潜在客户", "neutral"),
        ("contacted", "初步接触", "info"),
        ("sampling", "送样测试", "teal"),
        ("quoting", "报价谈判", "warning"),
        ("won", "定点量产", "success"),
        ("paused_lost", "暂停 / 流失", "danger"),
    ];
    let stages: Vec<Value> = stage_info
        .iter()
        .map(|(code, label, tone)| {
            let group: Vec<Value> = customers
                .iter()
                .filter(|row| row.get("stage").and_then(Value::as_str) == Some(*code))
                .cloned()
                .collect();
            json!({"code":code,"label":label,"tone":tone,"count":group.len(),"customers":group})
        })
        .collect();
    let mut todos = Vec::new();
    for row in latest_follow_ups
        .iter()
        .filter(|row| {
            row.get("next_date")
                .and_then(Value::as_str)
                .map(|date| date <= current_day)
                .unwrap_or(false)
        })
        .take(8)
    {
        let due = row
            .get("next_date")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let subtitle = if due == current_day {
            "今日到期".to_string()
        } else {
            format!("已超期至 {due}")
        };
        todos.push(json!({"id":format!("follow-{}",row["id"]),"title":format!("跟进 {}",row["customer_name"]),"subtitle":subtitle,"due_date":due,"tone":if due==current_day{"warning"}else{"danger"},"entity":"follow_ups","entity_id":row["id"]}));
    }
    for row in statements
        .iter()
        .filter(|row| row.get("pay_status").and_then(Value::as_str) != Some("settled"))
        .take(8)
    {
        todos.push(json!({"id":format!("pay-{}",row["id"]),"title":format!("回款 {}",row["no"]),"subtitle":format!("{} · {}",row["customer_name"],if row["pay_status"]=="overdue"{"已逾期"}else{"待跟进"}),"due_date":row["promise_date"],"tone":if row["pay_status"]=="overdue"{"danger"}else{"warning"},"entity":"statements","entity_id":row["id"]}));
    }
    Ok(
        json!({"metrics":{"monthly_sales_cents":monthly_sales,"monthly_received_cents":monthly_received,"unpaid_cents":unpaid,"overdue_count":overdue_count},"todos":todos,"stages":stages}),
    )
}

#[tauri::command]
fn get_dashboard(state: State<'_, AppState>) -> Result<Value, String> {
    let guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    dashboard_data(conn, &today())
}

#[tauri::command]
fn list_background_tasks(state: State<'_, AppState>) -> Result<Vec<BackgroundTask>, String> {
    state.tasks.snapshot()
}

#[tauri::command]
fn backup_now(state: State<'_, AppState>) -> Result<BackupResult, String> {
    let _task = state.tasks.begin("backup", "创建账套备份")?;
    let guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    let master = *state.protection_master.lock().map_err(to_error)?;
    backup_database(conn, &state.root, master.as_ref())
}

fn scan_backup_files(root: &Path, known: &mut HashMap<String, BackupEntry>) -> Result<(), String> {
    let Ok(entries) = fs::read_dir(root.join("backup")) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !is_backup_file_name(&path) {
            continue;
        }
        let path_string = path.to_string_lossy().to_string();
        if known.contains_key(&path_string) {
            continue;
        }
        let created_at = entry
            .metadata()
            .ok()
            .and_then(|meta| meta.modified().ok())
            .map(|time| chrono::DateTime::<Local>::from(time).to_rfc3339())
            .unwrap_or_default();
        let (checksum, size_bytes, result) = match fs::read(&path) {
            Ok(bytes) => (
                hex::encode(Sha256::digest(&bytes)),
                bytes.len() as i64,
                "untracked".to_string(),
            ),
            Err(_) => (String::new(), 0, "unreadable".to_string()),
        };
        known.insert(
            path_string.clone(),
            BackupEntry {
                path: path_string,
                checksum,
                size_bytes,
                schema_version: SCHEMA_VERSION,
                result,
                created_at,
                exists: path.is_file(),
            },
        );
    }
    Ok(())
}

fn finalize_backup_entries(known: &mut HashMap<String, BackupEntry>) -> Vec<BackupEntry> {
    for entry in known.values_mut() {
        if !entry.exists || entry.result == "untracked" || entry.result == "unreadable" {
            continue;
        }
        entry.result = match fs::read(&entry.path) {
            Ok(bytes) if hex::encode(Sha256::digest(&bytes)) == entry.checksum => {
                "success".to_string()
            }
            _ => "checksum_mismatch".to_string(),
        };
    }
    let mut output: Vec<BackupEntry> = known.drain().map(|(_, entry)| entry).collect();
    output.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| b.path.cmp(&a.path))
    });
    output
}

#[tauri::command]
fn list_backups(state: State<'_, AppState>) -> Result<Vec<BackupEntry>, String> {
    let _task = state.tasks.begin("read", "检查备份文件")?;
    let guard = state.db.lock().map_err(to_error)?;
    let mut known = HashMap::<String, BackupEntry>::new();
    for entry in load_backup_manifest(&state.root).unwrap_or_default() {
        let exists = Path::new(&entry.path).is_file();
        known.insert(
            entry.path.clone(),
            BackupEntry {
                path: entry.path,
                checksum: entry.checksum,
                size_bytes: entry.size_bytes,
                schema_version: entry.schema_version,
                result: if exists {
                    "success".to_string()
                } else {
                    "missing".to_string()
                },
                created_at: entry.created_at,
                exists,
            },
        );
    }
    if let Some(conn) = guard.as_ref() {
        let mut stmt = conn
            .prepare("SELECT path,checksum,size_bytes,schema_version,result,created_at FROM backup_records ORDER BY created_at DESC")
            .map_err(to_error)?;
        let rows = stmt
            .query_map([], |row| {
                let path: String = row.get(0)?;
                Ok(BackupEntry {
                    exists: Path::new(&path).is_file(),
                    path,
                    checksum: row.get(1)?,
                    size_bytes: row.get(2)?,
                    schema_version: row.get(3)?,
                    result: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })
            .map_err(to_error)?;
        for row in rows {
            let entry = row.map_err(to_error)?;
            known.insert(entry.path.clone(), entry);
        }
    }
    scan_backup_files(&state.root, &mut known)?;
    Ok(finalize_backup_entries(&mut known))
}

fn restore_drill_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RestoreDrillRecord> {
    Ok(RestoreDrillRecord {
        id: row.get(0)?,
        backup_path: row.get(1)?,
        backup_checksum: row.get(2)?,
        backup_created_at: row.get(3)?,
        started_at: row.get(4)?,
        finished_at: row.get(5)?,
        duration_ms: row.get(6)?,
        result: row.get(7)?,
        error_summary: row.get(8)?,
    })
}

fn list_restore_drills_conn(
    conn: &Connection,
    limit: i64,
) -> Result<Vec<RestoreDrillRecord>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id,backup_path,backup_checksum,backup_created_at,started_at,finished_at,duration_ms,result,error_summary FROM restore_drills ORDER BY started_at DESC,id DESC LIMIT ?1",
        )
        .map_err(to_error)?;
    let rows = stmt
        .query_map([limit.max(1)], restore_drill_from_row)
        .map_err(to_error)?;
    rows.map(|row| row.map_err(to_error)).collect()
}

fn insert_restore_drill(
    conn: &Connection,
    backup_path: &str,
    backup_checksum: &str,
    backup_created_at: Option<&str>,
    started_at: &str,
    finished_at: &str,
    duration_ms: i64,
    result: &str,
    error_summary: &str,
) -> Result<i64, String> {
    conn.execute(
        "INSERT INTO restore_drills(backup_path,backup_checksum,backup_created_at,started_at,finished_at,duration_ms,result,error_summary) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            backup_path,
            backup_checksum,
            backup_created_at,
            started_at,
            finished_at,
            duration_ms,
            result,
            error_summary
        ],
    )
    .map_err(to_error)?;
    Ok(conn.last_insert_rowid())
}

fn assessment_metric(
    key: &str,
    label: &str,
    value: Option<f64>,
    target: &str,
    status: &str,
    unit: &str,
    formula: &str,
    source: &str,
    window: &str,
    sample_size: i64,
) -> OperationalMetric {
    OperationalMetric {
        key: key.to_string(),
        label: label.to_string(),
        value,
        target: target.to_string(),
        status: status.to_string(),
        unit: unit.to_string(),
        formula: formula.to_string(),
        source: source.to_string(),
        window: window.to_string(),
        sample_size,
    }
}

fn at_most_status(value: Option<f64>, target: f64) -> &'static str {
    match value {
        Some(value) if value <= target => "pass",
        Some(_) => "fail",
        None => "insufficient_data",
    }
}

fn at_least_status(value: Option<f64>, target: f64) -> &'static str {
    match value {
        Some(value) if value >= target => "pass",
        Some(_) => "fail",
        None => "insufficient_data",
    }
}

fn timestamp_age_hours(value: &str) -> Option<f64> {
    let parsed = chrono::DateTime::parse_from_rfc3339(value).ok()?;
    let now = Local::now().fixed_offset();
    Some(((now - parsed).num_seconds().max(0) as f64) / 3600.0)
}

fn operational_data_completeness(conn: &Connection) -> Result<(i64, i64), String> {
    let checks = [
        ("customers", "SELECT COUNT(*),COALESCE(SUM(CASE WHEN TRIM(name)<>'' AND TRIM(stage)<>'' THEN 1 ELSE 0 END),0) FROM customers WHERE archived_at IS NULL"),
        ("materials", "SELECT COUNT(*),COALESCE(SUM(CASE WHEN TRIM(code)<>'' AND TRIM(base_resin)<>'' THEN 1 ELSE 0 END),0) FROM materials WHERE archived_at IS NULL"),
        ("samples", "SELECT COUNT(*),COALESCE(SUM(CASE WHEN TRIM(code)<>'' AND customer_id>0 AND material_id>0 THEN 1 ELSE 0 END),0) FROM samples WHERE archived_at IS NULL"),
        ("quotations", "SELECT COUNT(*),COALESCE(SUM(CASE WHEN TRIM(no)<>'' AND customer_id>0 AND material_id>0 AND price_cents>0 THEN 1 ELSE 0 END),0) FROM quotations WHERE archived_at IS NULL"),
        ("contracts", "SELECT COUNT(*),COALESCE(SUM(CASE WHEN TRIM(no)<>'' AND TRIM(seller_name)<>'' AND TRIM(buyer_name)<>'' THEN 1 ELSE 0 END),0) FROM contracts WHERE archived_at IS NULL"),
        ("orders", "SELECT COUNT(*),COALESCE(SUM(CASE WHEN TRIM(no)<>'' AND customer_id>0 AND material_id>0 AND qty_grams>0 AND price_cents>0 THEN 1 ELSE 0 END),0) FROM orders WHERE archived_at IS NULL"),
        ("statements", "SELECT COUNT(*),COALESCE(SUM(CASE WHEN TRIM(no)<>'' AND customer_id>0 AND TRIM(period_start)<>'' AND TRIM(period_end)<>'' THEN 1 ELSE 0 END),0) FROM statements WHERE archived_at IS NULL"),
        ("suppliers", "SELECT COUNT(*),COALESCE(SUM(CASE WHEN TRIM(name)<>'' THEN 1 ELSE 0 END),0) FROM suppliers WHERE archived_at IS NULL"),
        ("purchases", "SELECT COUNT(*),COALESCE(SUM(CASE WHEN TRIM(no)<>'' AND supplier_id>0 AND material_id>0 AND qty_grams>0 THEN 1 ELSE 0 END),0) FROM purchases WHERE archived_at IS NULL"),
        ("follow_ups", "SELECT COUNT(*),COALESCE(SUM(CASE WHEN customer_id>0 AND TRIM(content)<>'' AND TRIM(follow_date)<>'' THEN 1 ELSE 0 END),0) FROM follow_ups"),
    ];
    let mut total = 0_i64;
    let mut complete = 0_i64;
    for (_, sql) in checks {
        let (rows, valid): (i64, i64) = conn
            .query_row(sql, [], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(to_error)?;
        total += rows;
        complete += valid;
    }
    Ok((total, complete))
}

fn build_operational_assessment(conn: &Connection) -> Result<OperationalAssessment, String> {
    let now = Local::now();
    let today_value = now.date_naive().to_string();
    let month_start = (now.date_naive() - chrono::Duration::days(30)).to_string();
    let week_start = (now.date_naive() - chrono::Duration::days(7)).to_string();

    let mut follow_stmt = conn
        .prepare("SELECT customer_id,COALESCE(next_date,''),COALESCE(follow_date,''),id FROM follow_ups WHERE next_date IS NOT NULL AND next_date<>''")
        .map_err(to_error)?;
    let follow_rows = follow_stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(to_error)?;
    let mut latest_follow_ups: HashMap<i64, (String, String, i64)> = HashMap::new();
    for row in follow_rows {
        let (customer_id, next_date, follow_date, id) = row.map_err(to_error)?;
        let replace = latest_follow_ups
            .get(&customer_id)
            .map(|(_, previous_date, previous_id)| {
                follow_date > *previous_date || (follow_date == *previous_date && id > *previous_id)
            })
            .unwrap_or(true);
        if replace {
            latest_follow_ups.insert(customer_id, (next_date, follow_date, id));
        }
    }
    let due_count = latest_follow_ups
        .values()
        .filter(|(next_date, _, _)| next_date <= &today_value)
        .count() as i64;
    let overdue_count = latest_follow_ups
        .values()
        .filter(|(next_date, _, _)| next_date < &today_value)
        .count() as i64;
    let overdue_rate = (due_count > 0).then(|| overdue_count as f64 * 100.0 / due_count as f64);

    let (backup_total, backup_success): (i64, i64) = conn
        .query_row(
            "SELECT COUNT(*),COALESCE(SUM(CASE WHEN result='success' THEN 1 ELSE 0 END),0) FROM backup_records WHERE created_at>=?1",
            [week_start.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(to_error)?;
    let schedule_enabled = conn
        .query_row(
            "SELECT value FROM settings WHERE key='backup_schedule_enabled'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(to_error)?
        .map(|value| matches!(value.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(true);
    let schedule_days = conn
        .query_row(
            "SELECT value FROM settings WHERE key='backup_schedule_days'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(to_error)?
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(1)
        .clamp(1, 30);
    let expected_backups = if schedule_enabled {
        ((7 + schedule_days - 1) / schedule_days).max(1)
    } else {
        backup_total
    };
    let backup_success_rate = (expected_backups > 0)
        .then(|| (backup_success as f64 * 100.0 / expected_backups as f64).min(100.0));

    let (drill_total, drill_success): (i64, i64) = conn
        .query_row(
            "SELECT COUNT(*),COALESCE(SUM(CASE WHEN result='success' THEN 1 ELSE 0 END),0) FROM restore_drills WHERE started_at>=?1",
            [month_start.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(to_error)?;
    let restore_success_rate =
        (drill_total > 0).then(|| drill_success as f64 * 100.0 / drill_total as f64);
    let latest_backup_created_at: Option<String> = conn
        .query_row(
            "SELECT created_at FROM backup_records WHERE result='success' ORDER BY created_at DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(to_error)?;
    let rpo_hours = latest_backup_created_at
        .as_deref()
        .and_then(timestamp_age_hours);
    let rto_minutes: Option<f64> = conn
        .query_row(
            "SELECT duration_ms FROM restore_drills WHERE result='success' AND duration_ms IS NOT NULL ORDER BY started_at DESC,id DESC LIMIT 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(to_error)?
        .map(|duration| duration as f64 / 60_000.0);
    let (complete_rows, total_rows) = operational_data_completeness(conn)?;
    let completeness = (total_rows > 0).then(|| complete_rows as f64 * 100.0 / total_rows as f64);
    let latest_restore_drill = list_restore_drills_conn(conn, 1)?.into_iter().next();

    let metrics = vec![
        assessment_metric(
            "follow_up_overdue_rate",
            "跟进超期率",
            overdue_rate,
            "≤ 5%",
            at_most_status(overdue_rate, 5.0),
            "%",
            "最新跟进记录中，已到期且未完成的任务数 / 已到期任务总数",
            "follow_ups（按客户取最新 next_date）",
            "最近 30 天口径",
            due_count,
        ),
        assessment_metric(
            "backup_success_rate",
            "备份成功率",
            backup_success_rate,
            "≥ 99%",
            at_least_status(backup_success_rate, 99.0),
            "%",
            "通过校验的备份数 / 计划应执行备份数",
            "backup_records + backup_schedule settings",
            "最近 7 天口径",
            expected_backups,
        ),
        assessment_metric(
            "restore_success_rate",
            "还原成功率",
            restore_success_rate,
            "100%",
            at_least_status(restore_success_rate, 100.0),
            "%",
            "恢复演练完整性检查通过次数 / 恢复演练次数",
            "restore_drills",
            "最近 30 天口径",
            drill_total,
        ),
        assessment_metric(
            "rpo_hours",
            "最近备份年龄（RPO）",
            rpo_hours,
            "≤ 24 小时",
            at_most_status(rpo_hours, 24.0),
            "小时",
            "当前时间 - 最近一次校验通过备份创建时间",
            "backup_records",
            "当前时点",
            if latest_backup_created_at.is_some() {
                1
            } else {
                0
            },
        ),
        assessment_metric(
            "rto_minutes",
            "最近恢复耗时（RTO）",
            rto_minutes,
            "≤ 30 分钟",
            at_most_status(rto_minutes, 30.0),
            "分钟",
            "最近一次成功恢复演练的临时库校验耗时",
            "restore_drills.duration_ms",
            "当前时点",
            if rto_minutes.is_some() { 1 } else { 0 },
        ),
        assessment_metric(
            "data_completeness",
            "数据完整率",
            completeness,
            "≥ 98%",
            at_least_status(completeness, 98.0),
            "%",
            "已填写必填字段的记录数 / 应填写记录数",
            "核心业务表必填字段抽查",
            "当前全量口径",
            total_rows,
        ),
    ];
    Ok(OperationalAssessment {
        generated_at: now_iso(),
        window_start: month_start,
        window_end: today_value,
        metrics,
        latest_restore_drill,
        demo_only: false,
    })
}

#[tauri::command]
fn run_restore_drill(state: State<'_, AppState>, backup_path: String) -> Result<Value, String> {
    let _task = state.tasks.begin("restore", "运行恢复演练")?;
    ensure_dirs(&state.root)?;
    let started_at = now_iso();
    let started = Instant::now();
    let master = *state.protection_master.lock().map_err(to_error)?;
    ensure_restore_drill_unlocked(&state.root, master.as_ref())?;
    let guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    let backup_path = backup_path.trim().to_string();
    if backup_path.is_empty() {
        return Err("请选择要演练的备份文件".into());
    }
    let backup_checksum = fs::read(&backup_path)
        .map(|bytes| hex::encode(Sha256::digest(&bytes)))
        .unwrap_or_default();
    let backup_created_at = backup_created_at(&state.root, &backup_path, Some(conn));
    let verification =
        verify_restore_drill_file(&state.root, &backup_path, Some(conn), master.as_ref());
    let finished_at = now_iso();
    let duration_ms = verification
        .as_ref()
        .map(|value| value.duration_ms)
        .unwrap_or_else(|_| started.elapsed().as_millis() as i64);
    let (result, error_summary, checksum, created_at) = match &verification {
        Ok(value) => (
            "success",
            String::new(),
            value.checksum.clone(),
            value.backup_created_at.clone().or(backup_created_at),
        ),
        Err(error) => (
            if error.contains("尚未解锁") {
                "blocked"
            } else {
                "failure"
            },
            error.clone(),
            backup_checksum,
            backup_created_at,
        ),
    };
    let id = insert_restore_drill(
        conn,
        &backup_path,
        &checksum,
        created_at.as_deref(),
        &started_at,
        &finished_at,
        duration_ms,
        result,
        &error_summary,
    )?;
    if let Err(error) = verification {
        return Err(error);
    }
    Ok(json!({
        "id": id,
        "ok": true,
        "result": result,
        "backup_path": backup_path,
        "backup_checksum": checksum,
        "backup_created_at": created_at,
        "started_at": started_at,
        "finished_at": finished_at,
        "duration_ms": duration_ms,
        "error_summary": ""
    }))
}

#[tauri::command]
fn get_operational_metrics(state: State<'_, AppState>) -> Result<OperationalAssessment, String> {
    let _task = state.tasks.begin("read", "读取运营评估")?;
    let guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    build_operational_assessment(conn)
}

fn validate_cloud_backup_file_name(file_name: &str) -> Result<String, String> {
    let candidate = file_name.trim().replace('\\', "/");
    let safe = candidate
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    if safe.is_empty()
        || safe == "."
        || safe == ".."
        || safe.chars().any(|value| value.is_control())
        || !is_backup_file_name(Path::new(&safe))
    {
        return Err("云端备份文件名必须是 erp-*.db 或 erp-*.db.enc".into());
    }
    Ok(safe)
}

#[tauri::command]
fn read_backup_bytes(
    state: State<'_, AppState>,
    backup_path: String,
) -> Result<CloudBackupFile, String> {
    let _task = state.tasks.begin("backup", "读取云端备份上传内容")?;
    let source = PathBuf::from(&backup_path);
    let backup_root = fs::canonicalize(state.root.join("backup")).map_err(to_error)?;
    let source_canonical = fs::canonicalize(&source)
        .map_err(|error| format!("云端备份读取失败：{}", file_operation_error(&source, error)))?;
    if !source_canonical.starts_with(&backup_root) {
        return Err("云端备份来源必须位于当前账套的 backup 目录".into());
    }
    let file_name = validate_cloud_backup_file_name(
        source_canonical
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default(),
    )?;
    let bytes = fs::read(&source_canonical).map_err(|error| {
        format!(
            "云端备份读取失败：{}",
            file_operation_error(&source_canonical, error)
        )
    })?;
    if bytes.is_empty() || bytes.len() > MAX_CLOUD_BACKUP_BYTES {
        return Err("云端备份大小超出允许范围".into());
    }
    let guard = state.db.lock().map_err(to_error)?;
    let checksum = verify_backup_checksum(&state.root, &backup_path, &bytes, guard.as_ref())?;
    let created_at = guard
        .as_ref()
        .and_then(|conn| {
            conn.query_row(
                "SELECT created_at FROM backup_records WHERE path=?1 ORDER BY created_at DESC LIMIT 1",
                [&backup_path],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .ok()
            .flatten()
        })
        .unwrap_or_else(now_iso);
    Ok(CloudBackupFile {
        file_name,
        size_bytes: bytes.len() as i64,
        bytes,
        checksum,
        schema_version: SCHEMA_VERSION,
        created_at,
    })
}

#[tauri::command]
fn stage_cloud_backup(
    state: State<'_, AppState>,
    file_name: String,
    bytes: Vec<u8>,
    checksum: String,
    schema_version: Option<i64>,
    created_at: Option<String>,
) -> Result<Value, String> {
    let _task = state.tasks.begin("backup", "保存云端备份")?;
    if bytes.is_empty() || bytes.len() > MAX_CLOUD_BACKUP_BYTES {
        return Err("云端备份大小超出允许范围".into());
    }
    let file_name = validate_cloud_backup_file_name(&file_name)?;
    let expected = validate_hash(&checksum)?;
    let actual = hex::encode(Sha256::digest(&bytes));
    if actual != expected {
        return Err("云端备份 SHA-256 校验不匹配，已拒绝写入".into());
    }
    let protected = protection_config_path(&state.root).is_file();
    if protected && !bytes.starts_with(ENCRYPTED_MAGIC) {
        return Err("当前账套已启用保护模式，只能接收加密云端备份".into());
    }
    ensure_dirs(&state.root)?;
    let backup_root = state.root.join("backup");
    fs::create_dir_all(&backup_root).map_err(|error| {
        format!(
            "云端备份目录创建失败：{}",
            file_operation_error(&backup_root, error)
        )
    })?;
    let mut target = backup_root.join(&file_name);
    if target.exists() {
        let existing = fs::read(&target).map_err(|error| file_operation_error(&target, error))?;
        if hex::encode(Sha256::digest(&existing)) == expected {
            return Ok(json!({
                "path": target.to_string_lossy(),
                "checksum": expected,
                "size_bytes": existing.len() as i64,
                "deduplicated": true,
            }));
        }
        let stem = target
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("erp-cloud-backup");
        target = backup_root.join(format!(
            "erp-cloud-{}-{}",
            Local::now().format("%Y%m%d-%H%M%S"),
            stem
        ));
    }
    write_bytes_atomically(&target, &bytes)
        .map_err(|error| format!("云端备份写入失败：{error}。请检查 backup 目录权限后重试。"))?;
    let path = target.to_string_lossy().to_string();
    let entry = BackupManifestEntry {
        path: path.clone(),
        checksum: expected.clone(),
        size_bytes: bytes.len() as i64,
        schema_version: schema_version.unwrap_or(SCHEMA_VERSION),
        created_at: created_at.unwrap_or_else(now_iso),
    };
    if let Err(error) = register_backup_manifest(&state.root, entry.clone()) {
        let _ = remove_file_if_exists(&target);
        return Err(format!("云端备份清单写入失败：{error}"));
    }
    let guard = state.db.lock().map_err(to_error)?;
    if let Some(conn) = guard.as_ref() {
        if let Err(error) = conn.execute(
            "INSERT OR IGNORE INTO backup_records(path,checksum,size_bytes,schema_version,result,created_at) VALUES (?1,?2,?3,?4,'success',?5)",
            params![entry.path, entry.checksum, entry.size_bytes, entry.schema_version, entry.created_at],
        ) {
            return Err(format!(
                "云端备份文件已保存但本地登记失败：{error}；文件已保留于 {path}"
            ));
        }
    }
    Ok(json!({
        "path": path,
        "checksum": expected,
        "size_bytes": bytes.len() as i64,
        "deduplicated": false,
    }))
}

#[tauri::command]
fn shutdown(state: State<'_, AppState>) -> Result<Value, String> {
    let _task = state.tasks.begin("backup", "退出前备份与关闭")?;
    let mut guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    let master = *state.protection_master.lock().map_err(to_error)?;
    let backup = backup_database(conn, &state.root, master.as_ref())?;
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .map_err(to_error)?;
    drop(guard.take());
    if let Some(master) = master.as_ref() {
        encrypt_database_at_rest(&state.root, master)?;
    }
    write_exit_state(&state.root, "clean", Some(&backup.path))?;
    Ok(json!({"status":"clean","backup_path":backup.path}))
}

fn csv_value_to_string(value: Option<&Value>) -> String {
    value
        .map(|value| {
            if value.is_string() {
                value.as_str().unwrap_or_default().to_string()
            } else {
                value.to_string()
            }
        })
        .unwrap_or_default()
}

fn rows_to_csv_bytes(rows: &[Value]) -> Result<Vec<u8>, String> {
    let mut writer = csv::WriterBuilder::new()
        .has_headers(false)
        .from_writer(Vec::<u8>::new());
    if let Some(first) = rows.first().and_then(Value::as_object) {
        let headers: Vec<String> = first.keys().cloned().collect();
        writer
            .write_record(headers.iter())
            .map_err(|error| format!("CSV 内容生成失败：{error}"))?;
        for row in rows {
            let object = row
                .as_object()
                .ok_or_else(|| "导出记录格式错误".to_string())?;
            writer
                .write_record(
                    headers
                        .iter()
                        .map(|key| csv_value_to_string(object.get(key))),
                )
                .map_err(|error| format!("CSV 内容生成失败：{error}"))?;
        }
    }
    writer
        .flush()
        .map_err(|error| format!("CSV 内容生成失败：{error}"))?;
    writer
        .into_inner()
        .map_err(|error| format!("CSV 内容生成失败：{}", error.error()))
}

fn write_csv_payload(
    root: &Path,
    stem: &str,
    plain_bytes: &[u8],
    master: Option<&[u8; 32]>,
) -> Result<(String, bool), String> {
    ensure_dirs(root)?;
    let encrypted = master.is_some();
    let payload = if let Some(key) = master {
        encrypt_bytes(key, plain_bytes)?
    } else {
        plain_bytes.to_vec()
    };
    let suffix = if encrypted { ".csv.enc" } else { ".csv" };
    let path = root.join("export").join(format!("{stem}{suffix}"));
    write_bytes_atomically(&path, &payload)?;
    Ok((path.to_string_lossy().to_string(), encrypted))
}

fn write_entity_export(
    root: &Path,
    entity: &str,
    rows: &[Value],
    master: Option<&[u8; 32]>,
) -> Result<Value, String> {
    let csv_bytes = rows_to_csv_bytes(rows)?;
    let stamp = Local::now().format("%Y%m%d-%H%M%S%.3f").to_string();
    let (path, encrypted) =
        write_csv_payload(root, &format!("{entity}-{stamp}"), &csv_bytes, master)?;
    Ok(json!({"path":path,"rows":rows.len(),"encrypted":encrypted,"format":"csv"}))
}

fn ensure_export_unlocked(root: &Path, master: Option<&[u8; 32]>) -> Result<(), String> {
    if protection_config_path(root).exists() && master.is_none() {
        return Err("当前保护账套尚未解锁".into());
    }
    Ok(())
}

#[tauri::command]
fn export_entity(
    state: State<'_, AppState>,
    entity: String,
    format: Option<String>,
) -> Result<Value, String> {
    let _task = state.tasks.begin("export", "导出 CSV")?;
    if let Some(format) = format.as_deref() {
        if format != "csv" {
            return Err("当前版本仅支持 CSV 导出".into());
        }
    }
    let guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    let master = *state.protection_master.lock().map_err(to_error)?;
    ensure_export_unlocked(&state.root, master.as_ref())?;
    let rows = if entity == "attachments" {
        list_attachments_conn(conn, &state.root, master.as_ref(), None, None)?
    } else {
        list_entities_conn(conn, &entity)?
    };
    write_entity_export(&state.root, &entity, &rows, master.as_ref())
}

#[cfg(test)]
fn import_csv_to_connection(
    conn: &mut Connection,
    root: &Path,
    entity: &str,
    file_name: &str,
    bytes: &[u8],
    requested_strategy: &str,
) -> Result<Value, String> {
    import_csv_to_connection_with_master(
        conn,
        root,
        entity,
        file_name,
        bytes,
        requested_strategy,
        None,
    )
}

fn import_csv_to_connection_with_master(
    conn: &mut Connection,
    root: &Path,
    entity: &str,
    file_name: &str,
    bytes: &[u8],
    requested_strategy: &str,
    master: Option<&[u8; 32]>,
) -> Result<Value, String> {
    let strategy = requested_strategy.trim().to_lowercase();
    if !matches!(strategy.as_str(), "skip" | "rollback") {
        return Err("导入策略只能是 skip 或 rollback".into());
    }
    let checksum = hex::encode(Sha256::digest(&bytes));
    let batch_id = format!(
        "IMP-{}-{}",
        Local::now().format("%Y%m%d%H%M%S%.3f"),
        checksum.get(..8).unwrap_or("00000000")
    );
    let parsed = match parse_import_csv(entity, bytes) {
        Ok(value) => value,
        Err(error) => {
            record_import_batch(conn, &batch_id, entity, file_name, &checksum, 0, 0, 1)?;
            return Ok(
                json!({"batch_id":batch_id,"inserted":0,"skipped":0,"failed":1,"errors":[error],"failed_file":Value::Null,"failed_file_encrypted":false,"rolled_back":true,"preflight_ok":false,"strategy":strategy}),
            );
        }
    };
    let tx = conn.transaction().map_err(to_error)?;
    let mut inserted = 0_i64;
    let mut skipped = 0_i64;
    let mut failed = 0_i64;
    let mut failures: Vec<(usize, Vec<String>, String)> = Vec::new();
    for row in parsed.rows {
        if let Some(error) = row.error {
            failed += 1;
            failures.push((row.line, row.values, error));
            continue;
        }
        let data = row.data.expect("valid import row has data");
        match save_import_row(&tx, entity, &data) {
            Ok(_) => inserted += 1,
            Err(error) if is_duplicate_import_error(&error) => skipped += 1,
            Err(error) => {
                failed += 1;
                failures.push((row.line, row.values, error));
            }
        }
    }
    let failed_file =
        write_failed_import_file_with_master(root, &batch_id, &parsed.headers, &failures, master)?;
    let failed_file_encrypted = master.is_some() && failed_file.is_some();
    let error_messages: Vec<String> = failures
        .iter()
        .take(100)
        .map(|(line, _, error)| format!("第 {line} 行：{error}"))
        .collect();
    let truncated = failures.len() > error_messages.len();
    if strategy == "rollback" && failed > 0 {
        tx.rollback().map_err(to_error)?;
        record_import_batch(
            conn, &batch_id, entity, file_name, &checksum, 0, skipped, failed,
        )?;
        let mut errors = error_messages;
        if truncated {
            errors.push(format!(
                "另有 {} 行错误，详见失败行文件",
                failures.len() - 100
            ));
        }
        return Ok(
            json!({"batch_id":batch_id,"inserted":0,"skipped":skipped,"failed":failed,"errors":errors,"failed_file":failed_file,"failed_file_encrypted":failed_file_encrypted,"rolled_back":true,"preflight_ok":false,"strategy":strategy}),
        );
    }
    record_import_batch_tx(
        &tx, &batch_id, entity, file_name, &checksum, inserted, skipped, failed,
    )?;
    tx.commit().map_err(to_error)?;
    let mut errors = error_messages;
    if truncated {
        errors.push(format!(
            "另有 {} 行错误，详见失败行文件",
            failures.len() - 100
        ));
    }
    Ok(
        json!({"batch_id":batch_id,"inserted":inserted,"skipped":skipped,"failed":failed,"errors":errors,"failed_file":failed_file,"failed_file_encrypted":failed_file_encrypted,"rolled_back":false,"preflight_ok":failed == 0,"strategy":strategy}),
    )
}

#[tauri::command]
fn import_file(
    state: State<'_, AppState>,
    entity: String,
    file_name: String,
    bytes: Vec<u8>,
    strategy: Option<String>,
) -> Result<Value, String> {
    let _task = state.tasks.begin("import", "导入 CSV")?;
    if file_name.to_lowercase().ends_with(".xlsx") {
        return Err("当前离线导入器已支持 CSV；请在 Excel/WPS 中另存为 UTF-8 CSV 后导入。".into());
    }
    let mut guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_mut()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    let master = *state.protection_master.lock().map_err(to_error)?;
    ensure_export_unlocked(&state.root, master.as_ref())?;
    import_csv_to_connection_with_master(
        conn,
        &state.root,
        &entity,
        &file_name,
        &bytes,
        strategy.as_deref().unwrap_or("rollback"),
        master.as_ref(),
    )
}

fn decode_backup_bytes(
    path: &Path,
    bytes: &[u8],
    master: Option<&[u8; 32]>,
) -> Result<Vec<u8>, String> {
    let encrypted = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case("enc"))
        .unwrap_or(false);
    if encrypted {
        let master = master.ok_or_else(|| "当前保护账套尚未解锁".to_string())?;
        decrypt_bytes(master, bytes)
    } else {
        Ok(bytes.to_vec())
    }
}

fn remove_sqlite_sidecars(path: &Path) {
    for suffix in ["-wal", "-shm"] {
        let _ = fs::remove_file(path.with_file_name(format!(
                "{}{}",
                path.file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default(),
                suffix
            )));
    }
}

fn remove_sqlite_artifacts(path: &Path) {
    let _ = fs::remove_file(path);
    remove_sqlite_sidecars(path);
}

fn capture_current_database_snapshot(
    root: &Path,
    quarantine: bool,
) -> Result<(CurrentDatabaseSnapshot, Option<String>), String> {
    let candidates = [
        ("active", active_db_path(root)),
        ("active-wal", root.join("data").join("erp.db-wal")),
        ("active-shm", root.join("data").join("erp.db-shm")),
        ("encrypted", encrypted_db_path(root)),
    ];
    let mut snapshot = CurrentDatabaseSnapshot::default();
    let mut quarantine_path = None;
    let quarantine_stamp = format!(
        "{}-{}",
        Local::now().format("%Y%m%d-%H%M%S%.3f"),
        hex::encode(random_bytes::<8>())
    );
    if quarantine {
        fs::create_dir_all(root.join("backup")).map_err(|error| {
            format!(
                "损坏主库隔离目录创建失败：{}",
                file_operation_error(&root.join("backup"), error)
            )
        })?;
    }
    for (label, path) in candidates {
        if !path.exists() {
            continue;
        }
        let bytes = fs::read(&path).map_err(|error| {
            format!(
                "当前账套文件读取失败，未执行还原：{}",
                file_operation_error(&path, error)
            )
        })?;
        match label {
            "active" => snapshot.active = Some(bytes.clone()),
            "encrypted" => snapshot.encrypted = Some(bytes.clone()),
            _ => {}
        }
        if quarantine {
            let target = root
                .join("backup")
                .join(format!("restore-source-{quarantine_stamp}-{label}"));
            write_bytes_atomically(&target, &bytes)
                .map_err(|error| format!("损坏主库隔离副本写入失败：{error}；原文件保持不变。"))?;
            if quarantine_path.is_none() && matches!(label, "active" | "encrypted") {
                quarantine_path = Some(target.to_string_lossy().to_string());
            }
        }
    }
    Ok((snapshot, quarantine_path))
}

fn restore_database_snapshot(
    root: &Path,
    snapshot: &CurrentDatabaseSnapshot,
) -> Result<(), String> {
    let active = active_db_path(root);
    let encrypted = encrypted_db_path(root);
    remove_sqlite_sidecars(&active);
    match &snapshot.active {
        Some(bytes) => write_bytes_atomically(&active, bytes)?,
        None => remove_file_if_exists(&active)?,
    }
    match &snapshot.encrypted {
        Some(bytes) => write_bytes_atomically(&encrypted, bytes)?,
        None => remove_file_if_exists(&encrypted)?,
    }
    Ok(())
}

fn validate_database_file(path: &Path) -> Result<(), String> {
    let conn = open_database(path)?;
    let result = (|| {
        migrate(&conn)?;
        integrity_check(&conn)?;
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(to_error)?;
        Ok(())
    })();
    drop(conn);
    remove_sqlite_sidecars(path);
    result
}

fn verify_restore_drill_file(
    root: &Path,
    backup_path: &str,
    conn: Option<&Connection>,
    master: Option<&[u8; 32]>,
) -> Result<RestoreDrillVerification, String> {
    ensure_restore_drill_unlocked(root, master)?;
    let source = PathBuf::from(backup_path);
    if !source.is_file() {
        return Err("备份文件不存在".into());
    }
    let backup_root = fs::canonicalize(root.join("backup")).map_err(to_error)?;
    let source_canonical = fs::canonicalize(&source).map_err(to_error)?;
    if !source_canonical.starts_with(&backup_root) {
        return Err("恢复演练来源必须位于当前账套的 backup 目录".into());
    }
    let source_bytes = fs::read(&source).map_err(|error| {
        format!(
            "恢复演练读取备份失败：{}",
            file_operation_error(&source, error)
        )
    })?;
    let checksum = verify_backup_checksum(root, backup_path, &source_bytes, conn)?;
    let created_at = backup_created_at(root, backup_path, conn);
    let plain = decode_backup_bytes(&source, &source_bytes, master)?;
    let temp = root.join("data").join(format!(
        "erp.restore-drill-{}.tmp.db",
        hex::encode(random_bytes::<8>())
    ));
    remove_sqlite_artifacts(&temp);
    let started = Instant::now();
    let validation = (|| {
        write_bytes_atomically(&temp, &plain)
            .map_err(|error| format!("恢复演练临时文件写入失败：{error}"))?;
        validate_database_file(&temp).map_err(|error| format!("恢复演练完整性检查失败：{error}"))
    })();
    remove_sqlite_artifacts(&temp);
    validation?;
    Ok(RestoreDrillVerification {
        checksum,
        backup_created_at: created_at,
        duration_ms: started.elapsed().as_millis() as i64,
    })
}

fn replace_database_files(
    root: &Path,
    plain: &[u8],
    master: Option<&[u8; 32]>,
    snapshot: &CurrentDatabaseSnapshot,
) -> Result<Connection, String> {
    let data_root = root.join("data");
    let temp = data_root.join(format!(
        "erp.restore-{}.tmp.db",
        hex::encode(random_bytes::<8>())
    ));
    let encrypted_temp = data_root.join(format!(
        "erp.restore-{}.tmp.db.enc",
        hex::encode(random_bytes::<8>())
    ));
    remove_sqlite_artifacts(&temp);
    remove_file_if_exists(&encrypted_temp)?;
    if let Err(error) = write_bytes_atomically(&temp, plain) {
        remove_sqlite_artifacts(&temp);
        return Err(format!("还原临时文件写入失败：{error}"));
    }
    if let Err(error) = validate_database_file(&temp) {
        remove_sqlite_artifacts(&temp);
        return Err(format!("备份完整性检查失败，已拒绝还原：{error}"));
    }
    let migrated_plain = match fs::read(&temp) {
        Ok(bytes) => bytes,
        Err(error) => {
            remove_sqlite_artifacts(&temp);
            return Err(format!(
                "还原数据库读取失败：{}",
                file_operation_error(&temp, error)
            ));
        }
    };
    if let Some(master) = master {
        let encrypted = encrypt_bytes(master, &migrated_plain)?;
        if let Err(error) = write_bytes_atomically(&encrypted_temp, &encrypted) {
            remove_sqlite_artifacts(&temp);
            remove_file_if_exists(&encrypted_temp)?;
            return Err(format!("还原后的加密 shadow 写入失败：{error}"));
        }
    }

    let active = active_db_path(root);
    let encrypted = encrypted_db_path(root);
    let replacement: Result<Connection, String> = (|| {
        remove_sqlite_sidecars(&active);
        replace_file_atomically(&temp, &active)
            .map_err(|error| format!("当前主库替换失败：{error}"))?;
        if master.is_some() {
            replace_file_atomically(&encrypted_temp, &encrypted)
                .map_err(|error| format!("加密数据库 shadow 替换失败：{error}"))?;
        } else {
            remove_file_if_exists(&encrypted)
                .map_err(|error| format!("清理旧加密数据库 shadow 失败：{error}"))?;
        }
        let conn = open_database(&active)?;
        migrate(&conn)?;
        integrity_check(&conn)?;
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(to_error)?;
        Ok(conn)
    })();
    match replacement {
        Ok(conn) => {
            remove_sqlite_artifacts(&temp);
            remove_file_if_exists(&encrypted_temp)?;
            Ok(conn)
        }
        Err(error) => {
            remove_sqlite_artifacts(&temp);
            remove_file_if_exists(&encrypted_temp)?;
            let rollback = restore_database_snapshot(root, snapshot);
            match rollback {
                Ok(()) => Err(format!("{error}；已恢复还原前的账套文件")),
                Err(rollback_error) => {
                    Err(format!("{error}；恢复还原前账套失败：{rollback_error}"))
                }
            }
        }
    }
}

#[tauri::command]
fn restore_backup(state: State<'_, AppState>, backup_path: String) -> Result<Value, String> {
    let _task = state.tasks.begin("restore", "还原账套备份")?;
    ensure_dirs(&state.root)?;
    let source = PathBuf::from(&backup_path);
    if !source.is_file() {
        return Err("备份文件不存在".into());
    }
    let backup_root = fs::canonicalize(state.root.join("backup")).map_err(to_error)?;
    let source_canonical = fs::canonicalize(&source).map_err(to_error)?;
    if !source_canonical.starts_with(&backup_root) {
        return Err("还原来源必须位于当前账套的 backup 目录".into());
    }
    let source_bytes = fs::read(&source).map_err(to_error)?;
    let mut guard = state.db.lock().map_err(to_error)?;
    let (master, pre_backup, quarantine_path, source_checksum) = {
        let master = *state.protection_master.lock().map_err(to_error)?;
        if protection_config_path(&state.root).exists() && master.is_none() {
            return Err("当前保护账套尚未解锁，无法还原加密备份".into());
        }
        let source_checksum =
            verify_backup_checksum(&state.root, &backup_path, &source_bytes, guard.as_ref())?;
        if let Some(conn) = guard.as_ref() {
            let pre = backup_database(conn, &state.root, master.as_ref())?;
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                .map_err(to_error)?;
            if let Some(master) = master.as_ref() {
                sync_encrypted_database_shadow(&state.root, master)?;
            }
            (master, Some(pre), None, source_checksum)
        } else {
            let (_, quarantine_path) = capture_current_database_snapshot(&state.root, true)?;
            (master, None, quarantine_path, source_checksum)
        }
    };
    let (current_snapshot, _) = capture_current_database_snapshot(&state.root, false)?;
    let plain = decode_backup_bytes(&source, &source_bytes, master.as_ref())?;

    // Release SQLite handles before replacing files. A failed replacement can
    // then restore the captured bytes without fighting a live file lock.
    drop(guard.take());
    let reopened =
        match replace_database_files(&state.root, &plain, master.as_ref(), &current_snapshot) {
            Ok(conn) => conn,
            Err(error) => {
                let active = active_db_path(&state.root);
                if active.is_file() {
                    if let Ok(conn) = open_database(&active) {
                        if integrity_check(&conn).is_ok() {
                            *guard = Some(conn);
                        }
                    }
                }
                return Err(error);
            }
        };
    *guard = Some(reopened);
    write_exit_state(
        &state.root,
        "dirty",
        pre_backup
            .as_ref()
            .map(|backup| backup.path.as_str())
            .or(quarantine_path.as_deref()),
    )?;
    Ok(json!({
        "ok": true,
        "protected_backup": pre_backup.map(|backup| backup.path),
        "quarantine_path": quarantine_path,
        "source_checksum": source_checksum
    }))
}

#[tauri::command]
fn integrity_check_command(state: State<'_, AppState>) -> Result<Value, String> {
    let _task = state.tasks.begin("integrity", "运行数据库完整性检查")?;
    let guard = state.db.lock().map_err(to_error)?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| "数据库尚未初始化".to_string())?;
    integrity_check(conn)?;
    Ok(json!({"ok":true,"checked_at":now_iso()}))
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}
impl<T> Pipe for T {}

fn packaged_webview2_runtime(root: &Path) -> Option<PathBuf> {
    let runtime = root.join("runtime").join("webview2");
    runtime
        .join("msedgewebview2.exe")
        .is_file()
        .then_some(runtime)
}

#[cfg(windows)]
fn configure_packaged_webview2(root: &Path) -> Option<PathBuf> {
    let runtime = packaged_webview2_runtime(root)?;
    std::env::set_var("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER", &runtime);
    Some(runtime)
}

fn is_left_tray_activation(button: MouseButton, state: MouseButtonState) -> bool {
    button == MouseButton::Left && state == MouseButtonState::Up
}

fn main() {
    let root = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    #[cfg(windows)]
    let _packaged_webview2 = configure_packaged_webview2(&root);
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
                let _ = window.unminimize();
            }
        }))
        .setup(|app| {
            let quit = MenuItemBuilder::with_id("quit", "退出并备份").build(app)?;
            let show = MenuItemBuilder::with_id("show", "打开工作台").build(app)?;
            let menu = MenuBuilder::new(app).items(&[&show, &quit]).build()?;
            let icon = app
                .default_window_icon()
                .cloned()
                .ok_or_else(|| "缺少默认窗口图标")?;
            TrayIconBuilder::new()
                .icon(icon)
                .tooltip("改性塑料销售 ERP")
                .menu(&menu)
                .on_menu_event(|app, event| {
                    if event.id().as_ref() == "show" {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    } else if event.id().as_ref() == "quit" {
                        let _ = app.emit("erp://request-exit", ());
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button,
                        button_state,
                        ..
                    } = event
                    {
                        if !is_left_tray_activation(button, button_state) {
                            return;
                        }
                        if let Some(window) = tray.app_handle().get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;
            Ok(())
        })
        .manage(AppState::new(root))
        .invoke_handler(tauri::generate_handler![
            initialize,
            mark_ui_ready,
            set_protection,
            disable_protection,
            list_entities,
            list_entities_page,
            list_attachments,
            save_attachment,
            download_attachment,
            list_settings,
            save_settings,
            get_sync_status,
            list_pending_sync_events,
            mark_sync_events_synced,
            mark_sync_events_failed,
            list_dicts,
            save_dict,
            save_entity,
            duplicate_quotation,
            compare_quotations,
            convert_quotation_to_order,
            list_document_items,
            save_document_items,
            save_payment_allocations,
            download_template,
            delete_entity,
            transition_entity,
            get_dashboard,
            list_background_tasks,
            backup_now,
            list_backups,
            read_backup_bytes,
            stage_cloud_backup,
            shutdown,
            export_entity,
            import_file,
            restore_backup,
            integrity_check_command,
            run_restore_drill,
            get_operational_metrics
        ])
        .run(tauri::generate_context!())
        .expect("error while running ERP application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packaged_webview2_runtime_requires_the_runtime_executable() {
        let root = std::env::temp_dir().join(format!(
            "erp-webview2-runtime-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after Unix epoch")
                .as_nanos()
        ));
        let runtime = root.join("runtime").join("webview2");
        fs::create_dir_all(&runtime).expect("create runtime directory");

        assert_eq!(packaged_webview2_runtime(&root), None);
        fs::write(runtime.join("msedgewebview2.exe"), b"test").expect("create runtime marker");
        assert_eq!(packaged_webview2_runtime(&root), Some(runtime));

        fs::remove_dir_all(root).expect("remove runtime test directory");
    }

    fn memory_db() -> Connection {
        let conn = Connection::open_in_memory().expect("memory database");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("foreign keys");
        migrate(&conn).expect("migration");
        conn
    }

    #[test]
    fn migration_creates_required_tables_and_is_healthy() {
        let conn = memory_db();
        integrity_check(&conn).expect("integrity check");
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('customers','materials','orders','statements','audit_logs','backup_records','sync_events')", [], |row| row.get(0)).expect("table count");
        assert_eq!(count, 7);
        let dict_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM dicts", [], |row| row.get(0))
            .expect("dict count");
        assert!(dict_count >= 20);
    }

    #[test]
    fn automatic_sync_queue_is_opt_in_and_keeps_retry_state() {
        let mut conn = memory_db();

        let tx = conn.transaction().expect("disabled transaction");
        audit(
            &tx,
            "customers",
            1,
            "create",
            None,
            Some(json!({"id": 1, "name": "离线客户"})),
            None,
        )
        .expect("disabled audit");
        tx.commit().expect("disabled commit");
        let disabled_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sync_events", [], |row| row.get(0))
            .expect("disabled queue count");
        assert_eq!(disabled_count, 0);

        conn.execute(
            "UPDATE settings SET value='1' WHERE key='cloud_sync_enabled'",
            [],
        )
        .expect("enable sync");
        let tx = conn.transaction().expect("enabled transaction");
        audit(
            &tx,
            "customers",
            2,
            "create",
            None,
            Some(json!({"id": 2, "name": "待同步客户"})),
            None,
        )
        .expect("enabled audit");
        tx.commit().expect("enabled commit");

        let (event_id, payload): (String, String) = conn
            .query_row(
                "SELECT event_id,payload_json FROM sync_events WHERE synced_at IS NULL",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("pending event");
        assert_eq!(event_id.len(), 32);
        assert!(payload.contains("待同步客户"));

        conn.execute(
            "UPDATE sync_events SET retry_count=retry_count+1,last_error='offline' WHERE event_id=?1",
            [&event_id],
        )
        .expect("record failure");
        let retry_count: i64 = conn
            .query_row(
                "SELECT retry_count FROM sync_events WHERE event_id=?1 AND synced_at IS NULL",
                [&event_id],
                |row| row.get(0),
            )
            .expect("retry state");
        assert_eq!(retry_count, 1);

        conn.execute(
            "UPDATE sync_events SET synced_at=datetime('now'),last_error='' WHERE event_id=?1",
            [&event_id],
        )
        .expect("mark synced");
        let pending_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sync_events WHERE synced_at IS NULL",
                [],
                |row| row.get(0),
            )
            .expect("pending count");
        assert_eq!(pending_count, 0);
    }

    #[test]
    fn paged_entity_results_preserve_filters_and_bound_response_size() {
        let rows = (1..=105)
            .map(|id| {
                json!({
                    "id": id,
                    "name": format!("性能客户-{id}"),
                    "stage": if id % 2 == 0 { "lead" } else { "contacted" },
                })
            })
            .collect::<Vec<_>>();
        let filtered = filter_records(rows, Some("性能客户"), Some("lead"), Some("stage"));
        assert_eq!(filtered.len(), 52);
        let page = paginate_records(filtered, 3, 20).expect("page");
        assert_eq!(page["total"], 52);
        assert_eq!(page["page"], 3);
        assert_eq!(page["page_size"], 20);
        assert_eq!(page["rows"].as_array().expect("rows").len(), 12);

        let clamped = paginate_records((1..=150).map(|id| json!({"id": id})).collect(), 1, 1000)
            .expect("clamped page");
        assert_eq!(clamped["page_size"], 100);
        assert_eq!(clamped["rows"].as_array().expect("rows").len(), 100);

        let out_of_range = paginate_records(vec![json!({"id": 1})], i64::MAX, 100)
            .expect("large page must not overflow");
        assert_eq!(out_of_range["total"], 1);
        assert!(out_of_range["rows"].as_array().expect("rows").is_empty());
    }

    #[test]
    #[ignore = "release performance gate; run with cargo test --release --ignored"]
    fn customers_ten_thousand_list_p95_stays_under_three_hundred_ms() {
        use std::time::Instant;

        let conn = memory_db();
        let tx = conn.unchecked_transaction().expect("transaction");
        for id in 1..=10_000_i64 {
            tx.execute(
                "INSERT INTO customers (name,main_host,direction,material_system,stage,next_follow_date,notes,created_at,updated_at) VALUES (?1,'','',?2,'lead',NULL,'',?3,?3)",
                params![format!("性能基准客户-{id}"), "PP", "2026-09-16T00:00:00Z"],
            )
            .expect("customer row");
        }
        tx.commit().expect("commit");

        let mut samples = Vec::with_capacity(20);
        for _ in 0..25 {
            let started = Instant::now();
            let rows = list_entities_conn(&conn, "customers").expect("list customers");
            let page = paginate_records(rows, 1, 20).expect("page customers");
            assert_eq!(page["total"], 10_000);
            assert_eq!(page["rows"].as_array().expect("page rows").len(), 20);
            if samples.len() < 20 {
                samples.push(started.elapsed().as_secs_f64() * 1000.0);
            }
        }
        samples.sort_by(|left, right| left.partial_cmp(right).expect("finite duration"));
        let p95_index = ((samples.len() * 95).div_ceil(100)).saturating_sub(1);
        let p95_ms = samples[p95_index];
        println!(
            "{{\"dataset_rows\":10000,\"page_size\":20,\"samples\":{},\"p95_ms\":{:.3},\"target_ms\":300}}",
            samples.len(), p95_ms
        );
        assert!(
            p95_ms <= 300.0,
            "10,000-row customer list P95 {:.3} ms exceeded 300 ms",
            p95_ms
        );
    }

    #[test]
    fn task_registry_releases_raii_leases_and_keeps_snapshot_order() {
        let registry = TaskRegistry::new();
        let first = registry.begin("write", "第一个任务").expect("first lease");
        let second = registry
            .begin("backup", "第二个任务")
            .expect("second lease");
        let snapshot = registry.snapshot().expect("snapshot");
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].id, first.id);
        assert_eq!(snapshot[1].id, second.id);
        assert_eq!(snapshot[0].kind, "write");
        assert_eq!(snapshot[1].label, "第二个任务");

        drop(second);
        let remaining = registry.snapshot().expect("remaining snapshot");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, first.id);
        drop(first);
        assert!(registry.snapshot().expect("empty snapshot").is_empty());
    }

    #[test]
    fn schema_v5_adds_default_close_behavior_idempotently() {
        let conn = memory_db();
        assert_eq!(close_behavior_from_connection(&conn), "minimize_to_tray");
        conn.execute("DELETE FROM settings WHERE key='close_behavior'", [])
            .expect("remove close behavior");
        conn.execute("DELETE FROM schema_migrations WHERE version=5", [])
            .expect("remove v5 marker");
        migrate(&conn).expect("apply v5 migration");
        assert_eq!(close_behavior_from_connection(&conn), "minimize_to_tray");
        let marker_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version=5",
                [],
                |row| row.get(0),
            )
            .expect("v5 marker count");
        assert_eq!(marker_count, 1);
        migrate(&conn).expect("reapply v5 migration");
        let setting_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM settings WHERE key='close_behavior'",
                [],
                |row| row.get(0),
            )
            .expect("close behavior count");
        assert_eq!(setting_count, 1);
    }

    #[test]
    fn close_behavior_validation_normalizes_aliases_and_rejects_unknown_values() {
        assert_eq!(
            normalize_setting("close_behavior", "tray").expect("tray alias"),
            "minimize_to_tray"
        );
        assert_eq!(
            normalize_setting("close_behavior", "exit").expect("exit alias"),
            "confirm_exit"
        );
        assert!(normalize_setting("close_behavior", "sometimes").is_err());
    }

    #[test]
    fn tray_activation_only_restores_on_left_button_release() {
        assert!(is_left_tray_activation(
            MouseButton::Left,
            MouseButtonState::Up
        ));
        assert!(!is_left_tray_activation(
            MouseButton::Left,
            MouseButtonState::Down
        ));
        assert!(!is_left_tray_activation(
            MouseButton::Right,
            MouseButtonState::Up
        ));
        assert!(!is_left_tray_activation(
            MouseButton::Middle,
            MouseButtonState::Up
        ));
    }

    #[test]
    fn schema_v6_adds_scheduled_backup_settings_idempotently() {
        let conn = memory_db();
        assert_eq!(
            conn.query_row(
                "SELECT value FROM settings WHERE key='backup_schedule_enabled'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("schedule enabled"),
            "1"
        );
        assert_eq!(
            conn.query_row(
                "SELECT value FROM settings WHERE key='backup_schedule_days'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("schedule days"),
            "1"
        );
        conn.execute(
            "DELETE FROM settings WHERE key IN ('backup_schedule_enabled','backup_schedule_days')",
            [],
        )
        .expect("remove scheduled settings");
        conn.execute("DELETE FROM schema_migrations WHERE version=6", [])
            .expect("remove v6 marker");
        migrate(&conn).expect("apply v6 migration");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM settings WHERE key IN ('backup_schedule_enabled','backup_schedule_days')",
                [],
                |row| row.get(0),
            )
            .expect("scheduled setting count");
        assert_eq!(count, 2);
        migrate(&conn).expect("reapply v6 migration");
        let marker_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version=6",
                [],
                |row| row.get(0),
            )
            .expect("v6 marker count");
        assert_eq!(marker_count, 1);
    }

    #[test]
    fn schema_v7_adds_contracts_and_quotation_snapshots_idempotently() {
        let conn = memory_db();
        let table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('contracts','contract_items')",
                [],
                |row| row.get(0),
            )
            .expect("contract tables");
        assert_eq!(table_count, 2);
        let quote_columns: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('quotations') WHERE name IN ('seller_name','recipient_name','sender_name','recipient_contact','recipient_fax','cc','page_count','request_review','request_comment','quote_date','subject','price_note','adjustment_note','footer_address','footer_phone','footer_fax','footer_email')",
                [],
                |row| row.get(0),
            )
            .expect("quotation columns");
        assert_eq!(quote_columns, 17);
        conn.execute("DELETE FROM schema_migrations WHERE version=7", [])
            .expect("remove v7 marker");
        migrate(&conn).expect("reapply v7 migration");
        migrate(&conn).expect("keep v7 idempotent");
        let marker_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version=7",
                [],
                |row| row.get(0),
            )
            .expect("v7 marker count");
        assert_eq!(marker_count, 1);
    }

    #[test]
    fn schema_v8_adds_restore_drills_idempotently() {
        let conn = memory_db();
        conn.execute("DROP TABLE restore_drills", [])
            .expect("drop restore drills");
        conn.execute("DELETE FROM schema_migrations WHERE version=8", [])
            .expect("remove v8 marker");
        migrate(&conn).expect("apply v8 migration");
        let table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='restore_drills'",
                [],
                |row| row.get(0),
            )
            .expect("restore drills table");
        assert_eq!(table_count, 1);
        migrate(&conn).expect("reapply v8 migration");
        let marker_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version=8",
                [],
                |row| row.get(0),
            )
            .expect("v8 marker count");
        assert_eq!(marker_count, 1);
    }

    #[test]
    fn valid_restore_drill_does_not_change_active_database_or_leave_temps() {
        let root = std::env::temp_dir().join(format!(
            "erp-restore-drill-valid-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        ensure_dirs(&root).expect("directories");
        let conn = open_database(&active_db_path(&root)).expect("open");
        migrate(&conn).expect("migration");
        conn.execute(
            "INSERT INTO customers(name,stage,created_at,updated_at) VALUES ('演练客户','lead',?1,?1)",
            [now_iso()],
        )
        .expect("insert");
        let backup = backup_database(&conn, &root, None).expect("backup");
        let before = fs::read(active_db_path(&root)).expect("active before");
        let verification =
            verify_restore_drill_file(&root, &backup.path, Some(&conn), None).expect("drill");
        assert_eq!(verification.checksum, backup.checksum);
        assert!(verification.duration_ms >= 0);
        assert_eq!(
            fs::read(active_db_path(&root)).expect("active after"),
            before
        );
        let temporary_files: Vec<_> = fs::read_dir(root.join("data"))
            .expect("data directory")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("erp.restore-drill-")
            })
            .collect();
        assert!(temporary_files.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn restore_drill_rejects_checksum_mismatch_and_corrupt_backup() {
        let root = std::env::temp_dir().join(format!(
            "erp-restore-drill-rejection-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        ensure_dirs(&root).expect("directories");
        let conn = open_database(&active_db_path(&root)).expect("open");
        migrate(&conn).expect("migration");
        let backup = backup_database(&conn, &root, None).expect("backup");
        let mut changed = fs::read(&backup.path).expect("backup bytes");
        changed.push(0);
        fs::write(&backup.path, changed).expect("tamper backup");
        let checksum_error = verify_restore_drill_file(&root, &backup.path, Some(&conn), None)
            .expect_err("checksum mismatch");
        assert!(checksum_error.contains("校验和不匹配"));
        let corrupt = root.join("backup").join("erp-corrupt.db");
        fs::write(&corrupt, b"not a sqlite database").expect("corrupt backup");
        let corrupt_error =
            verify_restore_drill_file(&root, &corrupt.to_string_lossy(), Some(&conn), None)
                .expect_err("corrupt backup");
        assert!(corrupt_error.contains("完整性检查失败"));
        let temporary_files: Vec<_> = fs::read_dir(root.join("data"))
            .expect("data directory")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("erp.restore-drill-")
            })
            .collect();
        assert!(temporary_files.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn restore_drill_requires_an_unlocked_protection_master() {
        let root = std::env::temp_dir().join(format!(
            "erp-restore-drill-lock-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        ensure_dirs(&root).expect("directories");
        fs::write(protection_config_path(&root), b"{}").expect("protection marker");
        let error = ensure_restore_drill_unlocked(&root, None).expect_err("locked drill");
        assert!(error.contains("尚未解锁"));
        ensure_restore_drill_unlocked(&root, Some(&[7_u8; 32])).expect("unlocked drill");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn operational_assessment_aggregates_restore_and_completeness_metrics() {
        let conn = memory_db();
        let now = now_iso();
        let customer_id = conn
            .execute(
                "INSERT INTO customers(name,stage,created_at,updated_at) VALUES ('评估客户','lead',?1,?1)",
                [&now],
            )
            .expect("customer") as i64;
        conn.execute(
            "INSERT INTO follow_ups(customer_id,content,follow_date,next_date,created_at) VALUES (?1,'逾期跟进',?2,?3,?2)",
            params![customer_id, today(), (Local::now().date_naive() - chrono::Duration::days(2)).to_string()],
        )
        .expect("follow up");
        conn.execute(
            "INSERT INTO backup_records(path,checksum,size_bytes,schema_version,result,created_at) VALUES ('backup.db','abc',1,8,'success',?1)",
            [&now],
        )
        .expect("backup record");
        conn.execute(
            "INSERT INTO restore_drills(backup_path,backup_checksum,backup_created_at,started_at,finished_at,duration_ms,result,error_summary) VALUES ('backup.db','abc',?1,?1,?1,1200,'success','')",
            [&now],
        )
        .expect("drill record");
        let assessment = build_operational_assessment(&conn).expect("assessment");
        assert_eq!(assessment.metrics.len(), 6);
        assert!(assessment.latest_restore_drill.is_some());
        let rto = assessment
            .metrics
            .iter()
            .find(|metric| metric.key == "rto_minutes")
            .expect("rto metric");
        assert_eq!(rto.value, Some(0.02));
        let completeness = assessment
            .metrics
            .iter()
            .find(|metric| metric.key == "data_completeness")
            .expect("completeness metric");
        assert!(completeness.value.is_some());
    }

    #[test]
    fn contract_header_items_and_status_flow_preserve_print_snapshots() {
        let mut conn = memory_db();
        let tx = conn.transaction().expect("transaction");
        let contract_id = save_record_tx(
            &tx,
            "contracts",
            &json!({
                "no":"TZJP202609-18",
                "seller_name":"南京聚隆科技股份有限公司",
                "buyer_name":"台州市骏普塑模有限公司",
                "execution_place":"南京",
                "contract_date":"2026-09-18",
                "settlement_method":"款到发货",
                "packaging":"25KG/袋",
                "terms":"十项合同条款",
                "seller_address":"南京市高新技术开发区聚龙路8号",
                "seller_legal_representative":"刘曙阳",
                "seller_agent":"张迎波",
                "seller_bank":"中信银行建邺支行",
                "seller_account":"7329210182800049261",
                "buyer_address":"浙江省台州黄岩区北城街道锦川路8号",
                "buyer_bank":"台州银行股份有限公司黄岩工业园区支行",
                "buyer_account":"530166084800015",
                "status":"draft"
            }),
        )
        .expect("contract header");
        let total = save_document_items_tx(
            &tx,
            "contracts",
            contract_id,
            &[json!({
                "material_name":"PP",
                "model":"PI0-S27A[BK16452]",
                "manufacturer":"聚隆",
                "qty_grams":200000,
                "unit_price_cents":1180
            })],
        )
        .expect("contract items");
        assert_eq!(total, 236000);
        tx.commit().expect("commit contract");

        let contract = current_record(&conn, "contracts", contract_id).expect("contract row");
        assert_eq!(contract["item_total_cents"], 236000);
        assert_eq!(contract["item_count"], 1);
        assert_eq!(contract["seller_legal_representative"], "刘曙阳");
        assert_eq!(contract["seller_agent"], "张迎波");
        assert_eq!(contract["seller_account"], "7329210182800049261");
        assert_eq!(contract["buyer_account"], "530166084800015");
        let items = list_document_items_conn(&conn, "contracts", contract_id).expect("items");
        assert_eq!(items[0]["model"], "PI0-S27A[BK16452]");
        assert_eq!(items[0]["manufacturer"], "聚隆");
        let signed = transition_entity_conn(&mut conn, "contracts", contract_id, "signed", None)
            .expect("sign contract");
        assert_eq!(signed["status"], "signed");
        assert!(
            transition_entity_conn(&mut conn, "contracts", contract_id, "draft", None).is_err()
        );
    }

    #[test]
    fn quotation_header_and_items_preserve_fax_snapshot_fields() {
        let mut conn = memory_db();
        let tx = conn.transaction().expect("transaction");
        tx.execute(
            "INSERT INTO customers(name,created_at,updated_at) VALUES ('浙江零跑科技股份有限公司',?1,?1)",
            [now_iso()],
        )
        .expect("customer");
        let customer_id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO materials(code,base_resin,created_at,updated_at) VALUES ('PG4-S01A','PP',?1,?1)",
            [now_iso()],
        )
        .expect("material");
        let material_id = tx.last_insert_rowid();
        let quotation_id = save_record_tx(
            &tx,
            "quotations",
            &json!({
                "no":"零跑PP-GF20-0615",
                "customer_id":customer_id,
                "material_id":material_id,
                "price_cents":1060,
                "seller_name":"南京聚隆科技股份有限公司",
                "recipient_name":"浙江零跑科技股份有限公司",
                "sender_name":"销售部",
                "recipient_contact":"采购部",
                "recipient_fax":"0571-00000000",
                "cc":"项目组",
                "page_count":1,
                "request_review":true,
                "request_comment":false,
                "quote_date":"2026-06-15",
                "subject":"材料报价",
                "price_note":"未税、含运费",
                "adjustment_note":"随原材料价格波动调整",
                "footer_address":"南京高新技术开发区聚龙路 8 号",
                "footer_phone":"025-58840064",
                "footer_fax":"025-58746904",
                "footer_email":"julong@publicl.ptt.js.cn",
                "status":"draft"
            }),
        )
        .expect("quotation header");
        save_document_items_tx(
            &tx,
            "quotations",
            quotation_id,
            &[json!({
                "material_id":material_id,
                "material_category":"PP-GF20",
                "material_grade":"PG4-S01A",
                "manufacturer":"南京聚隆科技股份有限公司",
                "note":"未税、含运费",
                "qty_grams":1000,
                "unit_price_cents":1060
            })],
        )
        .expect("quotation items");
        tx.commit().expect("commit quotation");

        let quotation = current_record(&conn, "quotations", quotation_id).expect("quotation row");
        assert_eq!(quotation["sender_name"], "销售部");
        assert_eq!(quotation["recipient_fax"], "0571-00000000");
        assert_eq!(quotation["request_review"], true);
        assert_eq!(quotation["request_comment"], false);
        assert_eq!(quotation["footer_phone"], "025-58840064");
        let items = list_document_items_conn(&conn, "quotations", quotation_id).expect("items");
        assert_eq!(items[0]["material_category"], "PP-GF20");
        assert_eq!(items[0]["material_grade"], "PG4-S01A");
        assert_eq!(items[0]["note"], "未税、含运费");
    }

    #[test]
    fn quotation_actions_clone_compare_convert_and_reject_invalid_order_atomically() {
        let mut conn = memory_db();
        let tx = conn.transaction().expect("transaction");
        tx.execute(
            "INSERT INTO customers(name,created_at,updated_at) VALUES ('报价动作客户',?1,?1)",
            [now_iso()],
        )
        .expect("customer");
        let customer_id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO materials(code,base_resin,created_at,updated_at) VALUES ('QUOTE-A','PP',?1,?1)",
            [now_iso()],
        )
        .expect("material");
        let material_id = tx.last_insert_rowid();
        let quotation_id = save_record_tx(
            &tx,
            "quotations",
            &json!({
                "no":"QUOTE-A-001",
                "customer_id":customer_id,
                "material_id":material_id,
                "price_cents":1060,
                "moq_grams":1000,
                "seller_name":"供方",
                "recipient_name":"报价动作客户",
                "version":1,
                "status":"draft"
            }),
        )
        .expect("quotation");
        save_document_items_tx(
            &tx,
            "quotations",
            quotation_id,
            &[json!({
                "material_id":material_id,
                "material_category":"PP-GF20",
                "material_grade":"QUOTE-A",
                "qty_grams":1000,
                "unit_price_cents":1060
            })],
        )
        .expect("quotation item");
        tx.commit().expect("commit source");

        let tx = conn.transaction().expect("duplicate transaction");
        let duplicate_id = duplicate_quotation_tx(&tx, quotation_id).expect("duplicate quote");
        tx.commit().expect("commit duplicate");
        let duplicate = current_record(&conn, "quotations", duplicate_id).expect("duplicate row");
        assert_eq!(duplicate["no"], "QUOTE-A-001-V2");
        assert_eq!(duplicate["version"], 2);
        assert_eq!(duplicate["status"], "draft");
        assert_eq!(
            list_document_items_conn(&conn, "quotations", duplicate_id)
                .expect("duplicate items")
                .len(),
            1
        );

        let tx = conn.transaction().expect("edit transaction");
        save_document_items_tx(
            &tx,
            "quotations",
            duplicate_id,
            &[json!({
                "material_id":material_id,
                "material_category":"PP-GF20",
                "material_grade":"QUOTE-A",
                "qty_grams":1000,
                "unit_price_cents":1120
            })],
        )
        .expect("change duplicate item");
        tx.commit().expect("commit change");
        let comparison =
            compare_quotations_conn(&conn, quotation_id, duplicate_id).expect("compare quotes");
        assert_eq!(comparison["same"], false);
        assert_eq!(
            comparison["item_differences"]
                .as_array()
                .expect("item diff")
                .len(),
            1
        );

        let tx = conn.transaction().expect("convert transaction");
        let order_id =
            convert_quotation_to_order_tx(&tx, duplicate_id, "SO-QUOTE-A-001", Some("2026-10-01"))
                .expect("convert order");
        tx.commit().expect("commit order");
        let order = current_record(&conn, "orders", order_id).expect("order row");
        assert_eq!(order["quotation_id"], duplicate_id);
        assert_eq!(order["no"], "SO-QUOTE-A-001");
        assert_eq!(order["status"], "pending_confirm");
        assert_eq!(
            list_document_items_conn(&conn, "orders", order_id)
                .expect("order items")
                .len(),
            1
        );

        let before_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM orders", [], |row| row.get(0))
            .expect("order count");
        let tx = conn.transaction().expect("invalid convert transaction");
        assert!(convert_quotation_to_order_tx(
            &tx,
            duplicate_id,
            "SO-QUOTE-A-002",
            Some("bad-date")
        )
        .is_err());
        drop(tx);
        let after_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM orders", [], |row| row.get(0))
            .expect("order count after rollback");
        assert_eq!(after_count, before_count);
    }

    #[test]
    fn scheduled_backup_settings_validate_bounds_and_boolean_aliases() {
        assert_eq!(
            normalize_setting("backup_schedule_enabled", "true").expect("enabled alias"),
            "1"
        );
        assert_eq!(
            normalize_setting("backup_schedule_enabled", "off").expect("disabled alias"),
            "0"
        );
        assert_eq!(
            normalize_setting("backup_schedule_days", "30").expect("upper bound"),
            "30"
        );
        assert!(normalize_setting("backup_schedule_days", "0").is_err());
        assert!(normalize_setting("backup_schedule_days", "31").is_err());
        assert!(normalize_setting("backup_schedule_enabled", "maybe").is_err());
    }

    #[test]
    fn follow_up_migration_backfills_existing_reminders_once() {
        let conn = memory_db();
        conn.execute(
            "INSERT INTO customers(name,next_follow_date,created_at,updated_at) VALUES (?1,?2,?3,?3)",
            params!["迁移提醒客户", "2026-09-16", now_iso()],
        ).expect("legacy customer");
        conn.execute("DELETE FROM schema_migrations WHERE version=2", [])
            .expect("remove v2 marker");
        migrate(&conn).expect("apply v2 migration");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM follow_ups WHERE content='历史迁移：保留原下次跟进日期。'",
                [],
                |row| row.get(0),
            )
            .expect("backfilled follow up");
        assert_eq!(count, 1);
        let next_date: Option<String> = conn
            .query_row(
                "SELECT next_date FROM follow_ups WHERE content='历史迁移：保留原下次跟进日期。'",
                [],
                |row| row.get(0),
            )
            .expect("backfilled due date");
        assert_eq!(next_date.as_deref(), Some("2026-09-16"));
        migrate(&conn).expect("reapply migration");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM follow_ups WHERE content='历史迁移：保留原下次跟进日期。'",
                [],
                |row| row.get(0),
            )
            .expect("idempotent follow up");
        assert_eq!(count, 1);
    }

    #[test]
    fn state_machine_rejects_illegal_jumps_and_requires_reason_for_loss() {
        assert!(allowed_transition("customers", "lead", "contacted"));
        assert!(!allowed_transition("customers", "lead", "won"));
        assert!(allowed_transition("projects", "lead", "contacted"));
        assert!(!allowed_transition("projects", "lead", "won"));
        assert!(allowed_transition("quotations", "draft", "sent"));
        assert!(allowed_transition("quotations", "sent", "accepted"));
        assert!(!allowed_transition("quotations", "draft", "accepted"));
        assert!(allowed_transition("orders", "preparing", "delivered"));
        assert!(!allowed_transition(
            "orders",
            "pending_confirm",
            "completed"
        ));
        assert!(needs_reason("quoting", "paused_lost"));
        assert!(needs_reason("testing", "failed"));
        assert!(needs_reason("sent", "rejected"));
        assert!(!needs_reason("lead", "contacted"));
    }

    #[test]
    fn customer_transition_creates_follow_up_with_labels_due_date_and_audit() {
        let mut conn = memory_db();
        conn.execute(
            "UPDATE dicts SET label='线索池' WHERE type='customer_stage' AND value='lead'",
            [],
        )
        .expect("custom lead label");
        conn.execute(
            "UPDATE dicts SET label='已建立联系' WHERE type='customer_stage' AND value='contacted'",
            [],
        )
        .expect("custom contacted label");
        conn.execute(
            "INSERT INTO customers(name,stage,next_follow_date,created_at,updated_at) VALUES (?1,'lead',?2,?3,?3)",
            params!["阶段流转客户", "2026-10-02", now_iso()],
        )
        .expect("customer");
        let customer_id = conn.last_insert_rowid();

        let customer = transition_entity_conn(
            &mut conn,
            "customers",
            customer_id,
            "contacted",
            Some("首次电话沟通"),
        )
        .expect("legal transition");

        assert_eq!(customer["stage"], "contacted");
        let follow_up: (String, String, Option<String>) = conn
            .query_row(
                "SELECT content,follow_date,next_date FROM follow_ups WHERE customer_id=?1",
                [customer_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("automatic follow up");
        assert_eq!(
            follow_up.0,
            "客户阶段由“线索池”流转至“已建立联系”。 原因：首次电话沟通"
        );
        assert_eq!(follow_up.1, today());
        assert_eq!(follow_up.2.as_deref(), Some("2026-10-02"));
        let audit_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_logs WHERE object_type='customers' AND object_id=?1 AND event_type='transition'",
                [customer_id],
                |row| row.get(0),
            )
            .expect("transition audit");
        assert_eq!(audit_count, 1);
    }

    #[test]
    fn illegal_customer_transition_is_atomic() {
        let mut conn = memory_db();
        conn.execute(
            "INSERT INTO customers(name,stage,next_follow_date,created_at,updated_at) VALUES (?1,'lead',?2,?3,?3)",
            params!["非法流转客户", "2026-10-03", now_iso()],
        )
        .expect("customer");
        let customer_id = conn.last_insert_rowid();

        let error = transition_entity_conn(&mut conn, "customers", customer_id, "won", None)
            .expect_err("illegal transition must fail");

        assert!(error.contains("不允许"));
        let stage: String = conn
            .query_row(
                "SELECT stage FROM customers WHERE id=?1",
                [customer_id],
                |row| row.get(0),
            )
            .expect("preserved customer stage");
        assert_eq!(stage, "lead");
        let follow_up_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM follow_ups WHERE customer_id=?1",
                [customer_id],
                |row| row.get(0),
            )
            .expect("follow up count");
        let audit_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_logs WHERE object_type='customers' AND object_id=?1",
                [customer_id],
                |row| row.get(0),
            )
            .expect("audit count");
        assert_eq!(follow_up_count, 0);
        assert_eq!(audit_count, 0);
    }

    #[test]
    fn encrypted_payload_round_trips_and_rejects_wrong_key() {
        let key = [7_u8; 32];
        let wrong_key = [8_u8; 32];
        let plain = b"erp protected payload";
        let encrypted = encrypt_bytes(&key, plain).expect("encrypt");
        assert!(encrypted.starts_with(ENCRYPTED_MAGIC));
        assert_eq!(decrypt_bytes(&key, &encrypted).expect("decrypt"), plain);
        assert!(decrypt_bytes(&wrong_key, &encrypted).is_err());
    }

    #[test]
    fn direct_save_cannot_bypass_state_machine() {
        let mut conn = memory_db();
        let tx = conn.transaction().expect("transaction");
        let customer_id = save_record_tx(
            &tx,
            "customers",
            &json!({"name":"状态保存测试","stage":"lead"}),
        )
        .expect("customer");
        let illegal = save_record_tx(
            &tx,
            "customers",
            &json!({"id":customer_id,"name":"状态保存测试","stage":"won"}),
        );
        assert!(illegal.is_err());
        let preserved = save_record_tx(
            &tx,
            "customers",
            &json!({"id":customer_id,"name":"状态保存测试"}),
        )
        .expect("preserve stage");
        assert_eq!(preserved, customer_id);
        let stage: String = tx
            .query_row(
                "SELECT stage FROM customers WHERE id=?1",
                [customer_id],
                |row| row.get(0),
            )
            .expect("stage");
        assert_eq!(stage, "lead");
    }

    #[test]
    fn follow_ups_preserve_history_and_sync_the_latest_customer_due_date() {
        let mut conn = memory_db();
        let tx = conn.transaction().expect("transaction");
        let customer_id = save_record_tx(
            &tx,
            "customers",
            &json!({"name":"跟进测试客户","stage":"lead"}),
        )
        .expect("customer");
        save_record_tx(&tx, "follow_ups", &json!({"customer_id":customer_id,"content":"首轮沟通","follow_date":"2026-09-01","next_date":"2026-09-10"})).expect("first follow up");
        save_record_tx(&tx, "follow_ups", &json!({"customer_id":customer_id,"content":"确认测试安排","follow_date":"2026-09-05","next_date":"2026-09-16"})).expect("second follow up");
        tx.commit().expect("commit");
        let customer = list_entities_conn(&conn, "customers")
            .expect("customers")
            .into_iter()
            .find(|row| row["id"] == customer_id)
            .expect("customer row");
        assert_eq!(customer["next_follow_date"], "2026-09-16");
        let follow_ups = list_entities_conn(&conn, "follow_ups").expect("follow ups");
        assert_eq!(follow_ups.len(), 2);
        assert_eq!(follow_ups[0]["content"], "确认测试安排");
    }

    #[test]
    fn dashboard_uses_only_the_latest_follow_up_for_due_metrics() {
        let mut conn = memory_db();
        let tx = conn.transaction().expect("transaction");
        let customer_id = save_record_tx(
            &tx,
            "customers",
            &json!({"name":"工作台跟进客户","stage":"lead"}),
        )
        .expect("customer");
        save_record_tx(&tx, "follow_ups", &json!({"customer_id":customer_id,"content":"过期的历史计划","follow_date":"2026-09-01","next_date":"2026-09-10"})).expect("old follow up");
        let current_id = save_record_tx(&tx, "follow_ups", &json!({"customer_id":customer_id,"content":"新的跟进计划","follow_date":"2026-09-05","next_date":"2026-09-14"})).expect("current follow up");
        tx.commit().expect("commit");
        let dashboard =
            dashboard_data(&conn, "2026-09-13").expect("dashboard without overdue follow up");
        assert_eq!(dashboard["metrics"]["overdue_count"], 0);
        assert!(dashboard["todos"]
            .as_array()
            .expect("todos")
            .iter()
            .all(|todo| todo["entity"] != "follow_ups"));

        let tx = conn.transaction().expect("update transaction");
        save_record_tx(&tx, "follow_ups", &json!({"id":current_id,"customer_id":customer_id,"content":"新的跟进计划","follow_date":"2026-09-05","next_date":"2026-09-12"})).expect("update follow up");
        tx.commit().expect("update commit");
        let dashboard =
            dashboard_data(&conn, "2026-09-13").expect("dashboard with overdue follow up");
        assert_eq!(dashboard["metrics"]["overdue_count"], 1);
        let follow_up_todo = dashboard["todos"]
            .as_array()
            .expect("todos")
            .iter()
            .find(|todo| todo["entity"] == "follow_ups")
            .expect("follow up todo");
        assert_eq!(follow_up_todo["entity_id"], current_id);
    }

    #[test]
    fn sample_tests_keep_each_retest_as_a_separate_record() {
        let mut conn = memory_db();
        let tx = conn.transaction().expect("transaction");
        let customer_id = save_record_tx(
            &tx,
            "customers",
            &json!({"name":"样品测试客户","stage":"lead"}),
        )
        .expect("customer");
        let supplier_id =
            save_record_tx(&tx, "suppliers", &json!({"name":"样品测试供应商"})).expect("supplier");
        let material_id = save_record_tx(
            &tx,
            "materials",
            &json!({"code":"SAMPLE-TEST-001","base_resin":"PP","supplier_id":supplier_id}),
        )
        .expect("material");
        let sample_id = save_record_tx(&tx, "samples", &json!({"code":"S-TEST-001","customer_id":customer_id,"material_id":material_id,"status":"pending_send"})).expect("sample");
        save_record_tx(&tx, "sample_tests", &json!({"sample_id":sample_id,"test_date":"2026-09-01","test_item":"冲击","result":"不通过","conclusion":"需要复测","next_action":"调整配方"})).expect("first test");
        save_record_tx(&tx, "sample_tests", &json!({"sample_id":sample_id,"test_date":"2026-09-05","test_item":"冲击","result":"通过","conclusion":"复测通过","next_action":"登记结论"})).expect("retest");
        tx.commit().expect("commit");
        let tests = list_entities_conn(&conn, "sample_tests").expect("sample tests");
        assert_eq!(tests.len(), 2);
        assert_eq!(tests[0]["sample_code"], "S-TEST-001");
        assert_eq!(tests[0]["conclusion"], "复测通过");
    }

    #[test]
    fn statement_totals_are_derived_from_orders_and_payments() {
        let mut conn = memory_db();
        let tx = conn.transaction().expect("transaction");
        let customer_id =
            save_record_tx(&tx, "customers", &json!({"name":"测试客户","stage":"lead"}))
                .expect("customer");
        let supplier_id =
            save_record_tx(&tx, "suppliers", &json!({"name":"测试供应商"})).expect("supplier");
        let material_id = save_record_tx(
            &tx,
            "materials",
            &json!({"code":"TEST-001","base_resin":"PP","supplier_id":supplier_id}),
        )
        .expect("material");
        let order_id = save_record_tx(&tx, "orders", &json!({"no":"SO-TEST-001","customer_id":customer_id,"material_id":material_id,"qty_grams":1000,"price_cents":250,"delivery_date":today()})).expect("order");
        assert!(order_id > 0);
        let statement_id = save_record_tx(&tx, "statements", &json!({"no":"AR-TEST-001","customer_id":customer_id,"period_start":"2000-01-01","period_end":"2999-12-31","account_days":30,"promise_date":"2999-12-31"})).expect("statement");
        save_record_tx(&tx, "payments", &json!({"statement_id":statement_id,"amount_cents":100,"pay_date":"2026-09-12","method":"转账"})).expect("payment");
        tx.commit().expect("commit");
        let row = list_entities_conn(&conn, "statements")
            .expect("list")
            .into_iter()
            .find(|row| row["id"] == statement_id)
            .expect("statement row");
        assert_eq!(row["total_cents"], 250);
        assert_eq!(row["received_cents"], 100);
        assert_eq!(row["unpaid_cents"], 150);
        assert_eq!(row["pay_status"], "not_due");
    }

    #[test]
    fn backup_is_a_readable_consistent_snapshot() {
        let root = std::env::temp_dir().join(format!("erp-backup-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        ensure_dirs(&root).expect("directories");
        let conn = open_database(&root.join("data").join("erp.db")).expect("open");
        migrate(&conn).expect("migration");
        conn.execute(
            "INSERT INTO customers(name,created_at,updated_at) VALUES ('备份测试',?1,?1)",
            [now_iso()],
        )
        .expect("insert");
        let result = backup_database(&conn, &root, None).expect("backup");
        assert!(Path::new(&result.path).exists());
        assert_eq!(result.checksum.len(), 64);
        let copied = open_database(Path::new(&result.path)).expect("open backup");
        integrity_check(&copied).expect("backup integrity");
        let count: i64 = copied
            .query_row("SELECT COUNT(*) FROM customers", [], |row| row.get(0))
            .expect("count");
        assert_eq!(count, 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn protected_backup_is_encrypted_and_restorable() {
        let root =
            std::env::temp_dir().join(format!("erp-protected-backup-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        ensure_dirs(&root).expect("directories");
        let conn = open_database(&root.join("data").join("erp.db")).expect("open");
        migrate(&conn).expect("migration");
        conn.execute(
            "INSERT INTO customers(name,created_at,updated_at) VALUES ('保护备份测试',?1,?1)",
            [now_iso()],
        )
        .expect("insert");
        let master = [9_u8; 32];
        let result = backup_database(&conn, &root, Some(&master)).expect("backup");
        assert!(result.path.ends_with(".db.enc"));
        let encrypted = fs::read(&result.path).expect("read encrypted backup");
        assert!(encrypted.starts_with(ENCRYPTED_MAGIC));
        let plain = decrypt_bytes(&master, &encrypted).expect("decrypt backup");
        let restore_path = root.join("data").join("restore.db");
        fs::write(&restore_path, plain).expect("write restore");
        let restored = open_database(&restore_path).expect("open restore");
        integrity_check(&restored).expect("restore integrity");
        let count: i64 = restored
            .query_row("SELECT COUNT(*) FROM customers", [], |row| row.get(0))
            .expect("count");
        assert_eq!(count, 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn damaged_uninitialized_database_restores_from_manifest_and_keeps_quarantine() {
        let root =
            std::env::temp_dir().join(format!("erp-damaged-restore-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        ensure_dirs(&root).expect("directories");
        let conn = open_database(&active_db_path(&root)).expect("open");
        migrate(&conn).expect("migration");
        conn.execute(
            "INSERT INTO customers(name,created_at,updated_at) VALUES ('损坏恢复客户',?1,?1)",
            [now_iso()],
        )
        .expect("insert");
        let backup = backup_database(&conn, &root, None).expect("backup");
        drop(conn);

        fs::write(active_db_path(&root), b"corrupted sqlite payload").expect("corrupt active");
        let (snapshot, quarantine) =
            capture_current_database_snapshot(&root, true).expect("capture corrupt source");
        let source_bytes = fs::read(&backup.path).expect("source bytes");
        verify_backup_checksum(&root, &backup.path, &source_bytes, None)
            .expect("manifest checksum");
        let plain = decode_backup_bytes(Path::new(&backup.path), &source_bytes, None)
            .expect("decode plain backup");
        let restored = replace_database_files(&root, &plain, None, &snapshot)
            .expect("restore damaged database");
        integrity_check(&restored).expect("restored integrity");
        let count: i64 = restored
            .query_row(
                "SELECT COUNT(*) FROM customers WHERE name='损坏恢复客户'",
                [],
                |row| row.get(0),
            )
            .expect("restored customer");
        assert_eq!(count, 1);
        drop(restored);
        assert!(quarantine
            .as_deref()
            .map(Path::new)
            .is_some_and(Path::exists));
        assert!(load_backup_manifest(&root)
            .expect("manifest")
            .iter()
            .any(|entry| entry.path == backup.path));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn protected_restore_rewrites_encrypted_database_shadow() {
        let root = std::env::temp_dir().join(format!(
            "erp-protected-damaged-restore-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        ensure_dirs(&root).expect("directories");
        let conn = open_database(&active_db_path(&root)).expect("open");
        migrate(&conn).expect("migration");
        conn.execute(
            "INSERT INTO customers(name,created_at,updated_at) VALUES ('保护恢复客户',?1,?1)",
            [now_iso()],
        )
        .expect("insert");
        let master = [44_u8; 32];
        let backup = backup_database(&conn, &root, Some(&master)).expect("protected backup");
        drop(conn);

        fs::write(active_db_path(&root), b"corrupted protected sqlite").expect("corrupt active");
        fs::write(
            encrypted_db_path(&root),
            encrypt_bytes(&master, b"old corrupt shadow").expect("old shadow"),
        )
        .expect("write old shadow");
        let (snapshot, _) = capture_current_database_snapshot(&root, false).expect("snapshot");
        let encrypted_backup = fs::read(&backup.path).expect("encrypted backup bytes");
        let plain = decode_backup_bytes(Path::new(&backup.path), &encrypted_backup, Some(&master))
            .expect("decode protected backup");
        let restored = replace_database_files(&root, &plain, Some(&master), &snapshot)
            .expect("restore protected database");
        integrity_check(&restored).expect("restored integrity");
        drop(restored);
        let shadow = fs::read(encrypted_db_path(&root)).expect("new shadow");
        let shadow_plain = decrypt_bytes(&master, &shadow).expect("decrypt new shadow");
        let shadow_path = root.join("data").join("shadow-check.db");
        fs::write(&shadow_path, shadow_plain).expect("write shadow check");
        let shadow_conn = open_database(&shadow_path).expect("open shadow check");
        integrity_check(&shadow_conn).expect("shadow integrity");
        let count: i64 = shadow_conn
            .query_row(
                "SELECT COUNT(*) FROM customers WHERE name='保护恢复客户'",
                [],
                |row| row.get(0),
            )
            .expect("shadow customer");
        assert_eq!(count, 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn manifest_checksum_mismatch_is_rejected_before_restore() {
        let root =
            std::env::temp_dir().join(format!("erp-restore-checksum-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        ensure_dirs(&root).expect("directories");
        let conn = open_database(&active_db_path(&root)).expect("open");
        migrate(&conn).expect("migration");
        let backup = backup_database(&conn, &root, None).expect("backup");
        drop(conn);
        let mut bytes = fs::read(&backup.path).expect("backup bytes");
        bytes.push(0);
        let error = verify_backup_checksum(&root, &backup.path, &bytes, None)
            .expect_err("changed backup must be rejected");
        assert!(error.contains("校验和不匹配"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn backup_scan_works_without_an_open_database() {
        let root = std::env::temp_dir().join(format!(
            "erp-backup-scan-without-db-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        ensure_dirs(&root).expect("directories");
        let path = root.join("backup").join("erp-untracked.db");
        let conn = open_database(&root.join("data").join("source.db")).expect("source");
        migrate(&conn).expect("migration");
        conn.execute("VACUUM INTO ?1", [path.to_string_lossy().to_string()])
            .expect("write untracked backup");
        drop(conn);
        let mut known = HashMap::new();
        scan_backup_files(&root, &mut known).expect("scan backups");
        let entries = finalize_backup_entries(&mut known);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].result, "untracked");
        assert!(entries[0].exists);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn failed_restore_keeps_corrupt_source_and_cleans_restore_temps() {
        let root = std::env::temp_dir().join(format!(
            "erp-failed-restore-cleanup-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        ensure_dirs(&root).expect("directories");
        let original = b"corrupt active database".to_vec();
        fs::write(active_db_path(&root), &original).expect("write corrupt active");
        let source = root.join("backup").join("erp-invalid.db");
        fs::write(&source, b"not a sqlite database").expect("write invalid backup");
        let (snapshot, quarantine) =
            capture_current_database_snapshot(&root, true).expect("capture source");
        let error = replace_database_files(&root, b"not a sqlite database", None, &snapshot)
            .expect_err("invalid backup must fail");
        assert!(error.contains("完整性检查失败"));
        assert_eq!(
            fs::read(active_db_path(&root)).expect("active bytes"),
            original
        );
        assert!(quarantine
            .as_deref()
            .map(Path::new)
            .is_some_and(Path::exists));
        let restore_temps: Vec<_> = fs::read_dir(root.join("data"))
            .expect("data directory")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("erp.restore-")
            })
            .collect();
        assert!(restore_temps.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn backup_write_failure_cleans_snapshot_and_keeps_blocking_target_untouched() {
        let root =
            std::env::temp_dir().join(format!("erp-backup-write-failure-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        ensure_dirs(&root).expect("directories");
        let conn = open_database(&root.join("data").join("erp.db")).expect("open");
        migrate(&conn).expect("migration");
        conn.execute(
            "INSERT INTO customers(name,created_at,updated_at) VALUES ('失败路径测试',?1,?1)",
            [now_iso()],
        )
        .expect("insert");

        let snapshot = root.join("backup").join("forced-write-failure.snapshot");
        let target = root.join("backup").join("blocked-target.db");
        fs::create_dir_all(&target).expect("blocking directory");
        let error = backup_database_to_paths(&conn, &root, None, &snapshot, &target)
            .expect_err("blocked target must fail");
        assert!(error.contains("备份文件写入失败"));
        assert!(error.contains("重试"));
        assert!(!snapshot.exists());
        assert!(target.is_dir());
        let temporary_files: Vec<_> = fs::read_dir(root.join("backup"))
            .expect("backup directory")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp"))
            .collect();
        assert!(temporary_files.is_empty());
        drop(conn);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn backup_metadata_failure_preserves_verified_file_and_reports_retry_path() {
        let root = std::env::temp_dir().join(format!(
            "erp-backup-metadata-failure-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        ensure_dirs(&root).expect("directories");
        let conn = open_database(&root.join("data").join("erp.db")).expect("open");
        migrate(&conn).expect("migration");
        conn.execute(
            "INSERT INTO customers(name,created_at,updated_at) VALUES ('登记失败测试',?1,?1)",
            [now_iso()],
        )
        .expect("insert");
        conn.execute_batch(
            "CREATE TRIGGER reject_backup_record BEFORE INSERT ON backup_records BEGIN SELECT RAISE(ABORT, '模拟登记失败'); END;",
        )
        .expect("failure trigger");

        let snapshot = root.join("backup").join("forced-metadata-failure.snapshot");
        let target = root
            .join("backup")
            .join("preserved-after-metadata-failure.db");
        let error = backup_database_to_paths(&conn, &root, None, &snapshot, &target)
            .expect_err("metadata failure must be reported");
        assert!(error.contains("备份文件已生成但登记失败"));
        assert!(error.contains(target.to_string_lossy().as_ref()));
        assert!(target.is_file());
        let verified = open_database(&target).expect("preserved backup");
        integrity_check(&verified).expect("preserved backup integrity");
        assert!(!snapshot.exists());
        drop(verified);
        drop(conn);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn v3_migration_backfills_allocations_and_cost_history_idempotently() {
        let conn = memory_db();
        let now = now_iso();
        let supplier_id = conn
            .execute(
                "INSERT INTO suppliers(name,created_at,updated_at) VALUES ('迁移供应商',?1,?1)",
                [&now],
            )
            .expect("supplier") as i64;
        let material_id = conn.execute("INSERT INTO materials(code,base_resin,supplier_id,cost_cents,cost_date,created_at,updated_at) VALUES ('MIG-001','PP',?1,1234,'2026-09-01',?2,?2)", params![supplier_id, now]).expect("material") as i64;
        let customer_id = conn
            .execute(
                "INSERT INTO customers(name,created_at,updated_at) VALUES ('迁移客户',?1,?1)",
                [&now],
            )
            .expect("customer") as i64;
        let statement_id = conn.execute("INSERT INTO statements(no,customer_id,period_start,period_end,created_at,updated_at) VALUES ('AR-MIG-001',?1,'2026-09-01','2026-09-30',?2,?2)", params![customer_id, now]).expect("statement") as i64;
        let payment_id = conn.execute("INSERT INTO payments(statement_id,amount_cents,pay_date,created_at) VALUES (?1,500,'2026-09-10',?2)", params![statement_id, now]).expect("payment") as i64;
        conn.execute("DELETE FROM schema_migrations WHERE version=3", [])
            .expect("reset v3 marker");
        migrate(&conn).expect("apply v3");
        assert_eq!(
            conn.query_row(
                "SELECT allocated_amount_cents FROM payment_allocations WHERE payment_id=?1",
                [payment_id],
                |row| row.get::<_, i64>(0)
            )
            .expect("allocation"),
            500
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM cost_history WHERE material_id=?1",
                [material_id],
                |row| row.get::<_, i64>(0)
            )
            .expect("history"),
            1
        );
        migrate(&conn).expect("reapply v3");
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM payment_allocations WHERE payment_id=?1",
                [payment_id],
                |row| row.get::<_, i64>(0)
            )
            .expect("allocation count"),
            1
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM cost_history WHERE material_id=?1",
                [material_id],
                |row| row.get::<_, i64>(0)
            )
            .expect("history count"),
            1
        );
    }

    #[test]
    fn document_items_drive_totals_reservations_and_delivery_movements() {
        let mut conn = memory_db();
        let tx = conn.transaction().expect("transaction");
        let customer_id =
            save_record_tx(&tx, "customers", &json!({"name":"明细客户","stage":"lead"}))
                .expect("customer");
        let supplier_id =
            save_record_tx(&tx, "suppliers", &json!({"name":"明细供应商"})).expect("supplier");
        let material_id = save_record_tx(
            &tx,
            "materials",
            &json!({"code":"ITEM-001","base_resin":"PP","supplier_id":supplier_id}),
        )
        .expect("material");
        save_record_tx(
            &tx,
            "inventory",
            &json!({"material_id":material_id,"on_hand_grams":100000,"safety_grams":10000}),
        )
        .expect("inventory");
        let order_id = save_record_tx(&tx, "orders", &json!({"no":"SO-ITEM-001","customer_id":customer_id,"material_id":material_id,"qty_grams":1000,"price_cents":100,"delivery_date":"2026-09-13","status":"preparing"})).expect("order");
        let total = save_document_items_tx(&tx, "orders", order_id, &[json!({"material_id":material_id,"batch":"B-01","qty_grams":50000,"unit_price_cents":200}), json!({"material_id":material_id,"batch":"B-02","qty_grams":25000,"unit_price_cents":240})]).expect("order items");
        assert_eq!(total, 16000);
        let header: (i64, i64, i64) = tx
            .query_row(
                "SELECT qty_grams,price_cents,material_id FROM orders WHERE id=?1",
                [order_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("order header");
        assert_eq!(header, (75000, 200, material_id));
        let reserved: i64 = tx.query_row("SELECT COALESCE(SUM(reserved_grams),0) FROM stock_reservations WHERE order_id=?1 AND status='active'", [order_id], |row| row.get(0)).expect("reservation");
        assert_eq!(reserved, 75000);
        let delivery_id = save_record_tx(
            &tx,
            "deliveries",
            &json!({"no":"DN-ITEM-001","order_id":order_id,"sent_date":"2026-09-13"}),
        )
        .expect("delivery");
        tx.commit().expect("commit");
        let on_hand: i64 = conn
            .query_row(
                "SELECT on_hand_grams FROM inventory WHERE material_id=?1",
                [material_id],
                |row| row.get(0),
            )
            .expect("on hand");
        assert_eq!(on_hand, 25000);
        let released: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM stock_reservations WHERE order_id=?1 AND status='released'",
                [order_id],
                |row| row.get(0),
            )
            .expect("released");
        assert!(released >= 1);
        let movement: (i64, i64) = conn.query_row("SELECT qty_grams,source_id FROM inventory_movements WHERE movement_type='sale_issue' AND source_id IN (SELECT id FROM delivery_items WHERE delivery_id=?1)", [delivery_id], |row| Ok((row.get(0)?, row.get(1)?))).expect("movement");
        assert_eq!(movement.0, 50000);
    }

    #[test]
    fn inventory_edit_preserves_existing_movement_net() {
        let mut conn = memory_db();
        let tx = conn.transaction().expect("transaction");
        let supplier_id =
            save_record_tx(&tx, "suppliers", &json!({"name":"库存基线供应商"})).expect("supplier");
        let material_id = save_record_tx(
            &tx,
            "materials",
            &json!({"code":"INV-BASE-001","base_resin":"PP","supplier_id":supplier_id}),
        )
        .expect("material");
        let inventory_id = save_record_tx(
            &tx,
            "inventory",
            &json!({"material_id":material_id,"on_hand_grams":100000,"safety_grams":10000}),
        )
        .expect("inventory");
        record_movement(
            &tx,
            material_id,
            "purchase_receipt",
            20000,
            "B-BASE",
            "主仓",
            900,
            None,
        )
        .expect("movement");
        save_record_tx(&tx, "inventory", &json!({"id":inventory_id,"material_id":material_id,"on_hand_grams":110000,"safety_grams":10000})).expect("inventory edit");
        tx.commit().expect("commit");
        let opening: i64 = conn
            .query_row(
                "SELECT opening_grams FROM inventory WHERE id=?1",
                [inventory_id],
                |row| row.get(0),
            )
            .expect("opening");
        let on_hand: i64 = conn
            .query_row(
                "SELECT on_hand_grams FROM inventory WHERE id=?1",
                [inventory_id],
                |row| row.get(0),
            )
            .expect("on hand");
        assert_eq!(opening, 90000);
        assert_eq!(on_hand, 110000);
    }

    #[test]
    fn payment_allocations_can_split_one_payment_across_statements() {
        let mut conn = memory_db();
        let tx = conn.transaction().expect("transaction");
        let customer_id =
            save_record_tx(&tx, "customers", &json!({"name":"分配客户","stage":"lead"}))
                .expect("customer");
        let supplier_id =
            save_record_tx(&tx, "suppliers", &json!({"name":"分配供应商"})).expect("supplier");
        let material_id = save_record_tx(
            &tx,
            "materials",
            &json!({"code":"ALLOC-001","base_resin":"PP","supplier_id":supplier_id}),
        )
        .expect("material");
        save_record_tx(&tx, "orders", &json!({"no":"SO-ALLOC-001","customer_id":customer_id,"material_id":material_id,"qty_grams":1000,"price_cents":100,"delivery_date":"2026-09-01"})).expect("order one");
        save_record_tx(&tx, "orders", &json!({"no":"SO-ALLOC-002","customer_id":customer_id,"material_id":material_id,"qty_grams":1000,"price_cents":200,"delivery_date":"2026-09-02"})).expect("order two");
        let first = save_record_tx(&tx, "statements", &json!({"no":"AR-ALLOC-001","customer_id":customer_id,"period_start":"2026-09-01","period_end":"2026-09-01","promise_date":"2099-01-01"})).expect("statement one");
        let second = save_record_tx(&tx, "statements", &json!({"no":"AR-ALLOC-002","customer_id":customer_id,"period_start":"2026-09-02","period_end":"2026-09-02","promise_date":"2099-01-01"})).expect("statement two");
        let payment = save_record_tx(&tx, "payments", &json!({"statement_id":first,"amount_cents":300,"pay_date":"2026-09-13","items":[{"statement_id":first,"allocated_amount_cents":100},{"statement_id":second,"allocated_amount_cents":200}]})).expect("payment");
        tx.commit().expect("commit");
        let rows = list_entities_conn(&conn, "statements").expect("statements");
        let one = rows
            .iter()
            .find(|row| row["id"] == first)
            .expect("first row");
        let two = rows
            .iter()
            .find(|row| row["id"] == second)
            .expect("second row");
        assert_eq!(one["received_cents"], 100);
        assert_eq!(two["received_cents"], 200);
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM payment_allocations WHERE payment_id=?1",
                [payment],
                |row| row.get::<_, i64>(0)
            )
            .expect("allocation count"),
            2
        );
    }

    #[test]
    fn delivery_aggregates_same_material_before_writing_stock_movements() {
        let mut conn = memory_db();
        let tx = conn.transaction().expect("transaction");
        let customer_id = save_record_tx(
            &tx,
            "customers",
            &json!({"name":"合计出库客户","stage":"lead"}),
        )
        .expect("customer");
        let supplier_id =
            save_record_tx(&tx, "suppliers", &json!({"name":"合计出库供应商"})).expect("supplier");
        let material_id = save_record_tx(
            &tx,
            "materials",
            &json!({"code":"DELIVERY-AGG-001","base_resin":"PP","supplier_id":supplier_id}),
        )
        .expect("material");
        save_record_tx(
            &tx,
            "inventory",
            &json!({"material_id":material_id,"on_hand_grams":50000,"safety_grams":0}),
        )
        .expect("inventory");
        let order_id = save_record_tx(&tx, "orders", &json!({"no":"SO-DELIVERY-AGG-001","customer_id":customer_id,"material_id":material_id,"qty_grams":1000,"price_cents":100,"delivery_date":"2026-09-13","status":"preparing"})).expect("order");
        save_document_items_tx(&tx, "orders", order_id, &[
            json!({"material_id":material_id,"batch":"A","qty_grams":30000,"unit_price_cents":100}),
            json!({"material_id":material_id,"batch":"B","qty_grams":30000,"unit_price_cents":100}),
        ]).expect("order items");
        tx.commit().expect("setup commit");
        let tx = conn.transaction().expect("delivery transaction");
        let result = save_record_tx(
            &tx,
            "deliveries",
            &json!({"no":"DN-DELIVERY-AGG-001","order_id":order_id,"sent_date":"2026-09-13"}),
        );
        assert!(result.is_err());
        tx.rollback().expect("rollback");
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM deliveries", [], |row| row
                .get::<_, i64>(0))
                .expect("delivery count"),
            0
        );
        assert_eq!(
            conn.query_row(
                "SELECT on_hand_grams FROM inventory WHERE material_id=?1",
                [material_id],
                |row| row.get::<_, i64>(0)
            )
            .expect("on hand"),
            50000
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM inventory_movements WHERE movement_type='sale_issue'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .expect("sale issues"),
            0
        );
    }

    #[test]
    fn payment_allocation_cannot_exceed_statement_balance_and_current_edit_is_allowed() {
        let mut conn = memory_db();
        let tx = conn.transaction().expect("setup transaction");
        let customer_id = save_record_tx(
            &tx,
            "customers",
            &json!({"name":"回款余额客户","stage":"lead"}),
        )
        .expect("customer");
        let supplier_id =
            save_record_tx(&tx, "suppliers", &json!({"name":"回款余额供应商"})).expect("supplier");
        let material_id = save_record_tx(
            &tx,
            "materials",
            &json!({"code":"PAY-BAL-001","base_resin":"PP","supplier_id":supplier_id}),
        )
        .expect("material");
        save_record_tx(&tx, "orders", &json!({"no":"SO-PAY-BAL-001","customer_id":customer_id,"material_id":material_id,"qty_grams":1000,"price_cents":1000,"delivery_date":"2026-09-01"})).expect("order");
        let statement_id = save_record_tx(&tx, "statements", &json!({"no":"AR-PAY-BAL-001","customer_id":customer_id,"period_start":"2026-09-01","period_end":"2026-09-30","promise_date":"2099-01-01"})).expect("statement");
        let first_payment = save_record_tx(&tx, "payments", &json!({"statement_id":statement_id,"amount_cents":600,"pay_date":"2026-09-13","method":"转账"})).expect("first payment");
        tx.commit().expect("setup commit");

        let tx = conn.transaction().expect("edit transaction");
        replace_payment_allocations(
            &tx,
            first_payment,
            &[json!({"statement_id":statement_id,"allocated_amount_cents":600})],
        )
        .expect("same payment edit");
        tx.commit().expect("edit commit");

        let tx = conn.transaction().expect("over allocation transaction");
        let result = save_record_tx(
            &tx,
            "payments",
            &json!({"statement_id":statement_id,"amount_cents":500,"pay_date":"2026-09-13","method":"转账"}),
        );
        assert!(result.is_err());
        tx.rollback().expect("over allocation rollback");
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM payments", [], |row| row
                .get::<_, i64>(0))
                .expect("payment count"),
            1
        );
        let statement = list_entities_conn(&conn, "statements")
            .expect("statements")
            .into_iter()
            .find(|row| row["id"] == statement_id)
            .expect("statement row");
        assert_eq!(statement["received_cents"], 600);
        assert_eq!(statement["unpaid_cents"], 400);
    }

    #[test]
    fn status_entry_conditions_require_dates_batches_and_business_evidence() {
        let mut conn = memory_db();
        let tx = conn.transaction().expect("transaction");
        let customer_id = save_record_tx(
            &tx,
            "customers",
            &json!({"name":"状态条件客户","stage":"lead"}),
        )
        .expect("customer");
        let supplier_id =
            save_record_tx(&tx, "suppliers", &json!({"name":"状态条件供应商"})).expect("supplier");
        let material_id = save_record_tx(
            &tx,
            "materials",
            &json!({"code":"STATUS-ENTRY-001","base_resin":"PP","supplier_id":supplier_id}),
        )
        .expect("material");

        let sample_id = save_record_tx(&tx, "samples", &json!({"code":"S-STATUS-001","customer_id":customer_id,"material_id":material_id,"status":"pending_send"})).expect("sample");
        assert!(validate_status_entry(&tx, "samples", sample_id, "sent").is_err());
        tx.execute(
            "UPDATE samples SET batch='B-STATUS',sent_date='2026-09-13' WHERE id=?1",
            [sample_id],
        )
        .expect("sample send fields");
        validate_status_entry(&tx, "samples", sample_id, "sent").expect("sample sent fields");
        assert!(validate_status_entry(&tx, "samples", sample_id, "testing").is_err());
        tx.execute(
            "UPDATE samples SET test_items='冲击' WHERE id=?1",
            [sample_id],
        )
        .expect("sample test item");
        validate_status_entry(&tx, "samples", sample_id, "testing").expect("sample testing fields");
        assert!(validate_status_entry(&tx, "samples", sample_id, "passed").is_err());
        tx.execute("INSERT INTO sample_tests(sample_id,test_date,test_item,conclusion,created_at) VALUES (?1,'2026-09-13','冲击','通过',?2)", params![sample_id, now_iso()]).expect("sample conclusion");
        validate_status_entry(&tx, "samples", sample_id, "passed").expect("sample passed fields");
        assert!(validate_status_entry(&tx, "samples", sample_id, "failed").is_err());
        tx.execute(
            "UPDATE samples SET fail_reason='需调整配方' WHERE id=?1",
            [sample_id],
        )
        .expect("sample fail reason");
        validate_status_entry(&tx, "samples", sample_id, "failed").expect("sample failed fields");

        let order_without_date = save_record_tx(&tx, "orders", &json!({"no":"SO-STATUS-001","customer_id":customer_id,"material_id":material_id,"qty_grams":1000,"price_cents":100})).expect("order without date");
        assert!(validate_status_entry(&tx, "orders", order_without_date, "preparing").is_err());
        tx.execute(
            "UPDATE orders SET delivery_date='2026-09-13' WHERE id=?1",
            [order_without_date],
        )
        .expect("order date");
        validate_status_entry(&tx, "orders", order_without_date, "preparing")
            .expect("order preparing fields");
        assert!(validate_status_entry(&tx, "orders", order_without_date, "delivered").is_err());
        save_record_tx(
            &tx,
            "inventory",
            &json!({"material_id":material_id,"on_hand_grams":1000,"safety_grams":0}),
        )
        .expect("inventory");
        let delivery_id = save_record_tx(
            &tx,
            "deliveries",
            &json!({"no":"DN-STATUS-001","order_id":order_without_date,"sent_date":"2026-09-13"}),
        )
        .expect("delivery");
        validate_status_entry(&tx, "orders", order_without_date, "delivered")
            .expect("order delivered fields");
        tx.execute(
            "UPDATE deliveries SET sign_date='2026-09-14' WHERE id=?1",
            [delivery_id],
        )
        .expect("sign date");
        validate_status_entry(&tx, "orders", order_without_date, "signed")
            .expect("order signed fields");
        tx.commit().expect("commit");
    }

    #[test]
    fn reminder_setting_changes_statement_near_due_boundary() {
        let mut conn = memory_db();
        let tx = conn.transaction().expect("transaction");
        let customer_id = save_record_tx(
            &tx,
            "customers",
            &json!({"name":"提醒参数客户","stage":"lead"}),
        )
        .expect("customer");
        let supplier_id =
            save_record_tx(&tx, "suppliers", &json!({"name":"提醒参数供应商"})).expect("supplier");
        let material_id = save_record_tx(
            &tx,
            "materials",
            &json!({"code":"REMINDER-001","base_resin":"PP","supplier_id":supplier_id}),
        )
        .expect("material");
        let order_date = Local::now().date_naive().to_string();
        let promise_date = (Local::now().date_naive() + chrono::Duration::days(5)).to_string();
        save_record_tx(&tx, "orders", &json!({"no":"SO-REMINDER-001","customer_id":customer_id,"material_id":material_id,"qty_grams":1000,"price_cents":1000,"delivery_date":order_date})).expect("order");
        let statement_id = save_record_tx(&tx, "statements", &json!({"no":"AR-REMINDER-001","customer_id":customer_id,"period_start":"2000-01-01","period_end":"2999-12-31","promise_date":promise_date})).expect("statement");
        let initial: String = tx
            .query_row(
                "SELECT pay_status FROM statements WHERE id=?1",
                [statement_id],
                |row| row.get(0),
            )
            .expect("initial status");
        assert_eq!(initial, "not_due");
        tx.execute(
            "UPDATE settings SET value='7',updated_at=?1 WHERE key='reminder_days'",
            [now_iso()],
        )
        .expect("set reminder");
        refresh_statement(&tx, statement_id).expect("refresh statement");
        let updated: String = tx
            .query_row(
                "SELECT pay_status FROM statements WHERE id=?1",
                [statement_id],
                |row| row.get(0),
            )
            .expect("updated status");
        assert_eq!(updated, "near_due");
        tx.commit().expect("commit");
    }

    #[test]
    fn material_cost_changes_leave_history() {
        let mut conn = memory_db();
        let tx = conn.transaction().expect("transaction");
        let supplier_id =
            save_record_tx(&tx, "suppliers", &json!({"name":"成本供应商"})).expect("supplier");
        let material_id = save_record_tx(&tx, "materials", &json!({"code":"COST-001","base_resin":"PP","supplier_id":supplier_id,"cost_cents":1000,"cost_date":"2026-09-01"})).expect("material");
        save_record_tx(&tx, "materials", &json!({"id":material_id,"code":"COST-001","base_resin":"PP","supplier_id":supplier_id,"cost_cents":1200,"cost_date":"2026-09-10"})).expect("cost update");
        tx.commit().expect("commit");
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM cost_history WHERE material_id=?1",
                [material_id],
                |row| row.get::<_, i64>(0)
            )
            .expect("history count"),
            2
        );
        assert_eq!(
            conn.query_row(
                "SELECT cost_cents FROM cost_history WHERE material_id=?1 ORDER BY id DESC LIMIT 1",
                [material_id],
                |row| row.get::<_, i64>(0)
            )
            .expect("latest history"),
            1200
        );
    }

    #[test]
    fn csv_import_preflight_rejects_wrong_headers_and_normalizes_rows() {
        let wrong = "name,unexpected\n客户,值\n".as_bytes();
        let error = parse_import_csv("customers", wrong).expect_err("wrong header");
        assert!(error.contains("不支持的字段：unexpected"));

        let parsed = parse_import_csv("orders", b"\xEF\xBB\xBFprice_cents,no,customer_id,material_id,qty_grams\n2180,SO-CSV-001,1,2,1000\n").expect("valid csv");
        assert_eq!(parsed.headers[0], "price_cents");
        let data = parsed.rows[0].data.as_ref().expect("normalized row");
        assert_eq!(data["price_cents"], 2180);
        assert_eq!(data["no"], "SO-CSV-001");
    }

    #[test]
    fn csv_import_row_savepoint_removes_partial_business_record() {
        let mut conn = memory_db();
        let tx = conn.transaction().expect("transaction");
        let customer_id = save_record_tx(&tx, "customers", &json!({"name":"CSV 行级回滚客户"}))
            .expect("customer");
        let supplier_id = save_record_tx(&tx, "suppliers", &json!({"name":"CSV 行级回滚供应商"}))
            .expect("supplier");
        let material_id = save_record_tx(
            &tx,
            "materials",
            &json!({"code":"CSV-ROLLBACK-001","base_resin":"PP","supplier_id":supplier_id}),
        )
        .expect("material");
        let result = save_import_row(
            &tx,
            "samples",
            &json!({"code":"S-CSV-ROLLBACK-001","customer_id":customer_id,"material_id":material_id,"status":"sent"}),
        );
        assert!(result.is_err());
        let count: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM samples WHERE code='S-CSV-ROLLBACK-001'",
                [],
                |row| row.get(0),
            )
            .expect("sample count");
        assert_eq!(count, 0);
        tx.commit().expect("commit setup");
    }

    #[test]
    fn csv_import_duplicate_row_is_skipped_without_residue() {
        let mut conn = memory_db();
        let tx = conn.transaction().expect("transaction");
        save_import_row(&tx, "customers", &json!({"name":"CSV 重复客户"})).expect("first row");
        let duplicate = save_import_row(&tx, "customers", &json!({"name":"CSV 重复客户"}))
            .expect_err("duplicate row");
        assert!(is_duplicate_import_error(&duplicate));
        let count: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM customers WHERE name='CSV 重复客户'",
                [],
                |row| row.get(0),
            )
            .expect("customer count");
        assert_eq!(count, 1);
        tx.commit().expect("commit");
    }

    #[test]
    fn failed_csv_rows_are_written_with_line_and_error_columns() {
        let root =
            std::env::temp_dir().join(format!("erp-csv-failure-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let failures = vec![(
            3_usize,
            vec!["bad".to_string()],
            "字段 price_cents 必须是整数".to_string(),
        )];
        let path =
            write_failed_import_file(&root, "IMP-TEST", &["price_cents".to_string()], &failures)
                .expect("write report")
                .expect("report path");
        let content = fs::read_to_string(&path).expect("read report");
        assert!(content.contains("price_cents,line,error"));
        assert!(content.contains("bad,3,"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn csv_export_is_atomic_and_protected_exports_are_encrypted() {
        let plain_root =
            std::env::temp_dir().join(format!("erp-csv-export-plain-test-{}", std::process::id()));
        let protected_root = std::env::temp_dir().join(format!(
            "erp-csv-export-protected-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&plain_root);
        let _ = fs::remove_dir_all(&protected_root);
        let rows = vec![json!({"name":"导出客户","notes":"敏感导出内容"})];

        let plain_result =
            write_entity_export(&plain_root, "customers", &rows, None).expect("plain export");
        assert_eq!(plain_result["encrypted"], false);
        let plain_path = plain_result["path"].as_str().expect("plain path");
        assert!(plain_path.ends_with(".csv"));
        let plain_bytes = fs::read(plain_path).expect("plain bytes");
        assert!(String::from_utf8_lossy(&plain_bytes).contains("敏感导出内容"));

        let master = [17_u8; 32];
        let protected_result =
            write_entity_export(&protected_root, "customers", &rows, Some(&master))
                .expect("protected export");
        assert_eq!(protected_result["encrypted"], true);
        let protected_path = protected_result["path"].as_str().expect("protected path");
        assert!(protected_path.ends_with(".csv.enc"));
        let encrypted = fs::read(protected_path).expect("encrypted bytes");
        assert!(encrypted.starts_with(ENCRYPTED_MAGIC));
        let decrypted = decrypt_bytes(&master, &encrypted).expect("decrypt export");
        assert_eq!(decrypted, plain_bytes);
        assert!(decrypt_bytes(&[19_u8; 32], &encrypted).is_err());
        let export_entries: Vec<_> = fs::read_dir(protected_root.join("export"))
            .expect("export directory")
            .filter_map(Result::ok)
            .collect();
        assert_eq!(export_entries.len(), 1);
        assert!(export_entries[0]
            .file_name()
            .to_string_lossy()
            .ends_with(".csv.enc"));
        assert!(export_entries
            .iter()
            .all(|entry| !entry.file_name().to_string_lossy().ends_with(".tmp")));

        let _ = fs::remove_dir_all(&plain_root);
        let _ = fs::remove_dir_all(&protected_root);
    }

    #[test]
    fn protected_export_requires_an_unlocked_master_key() {
        let root =
            std::env::temp_dir().join(format!("erp-csv-export-lock-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        ensure_dirs(&root).expect("directories");
        fs::write(protection_config_path(&root), b"{}").expect("protection marker");
        let error = ensure_export_unlocked(&root, None).expect_err("locked export");
        assert_eq!(error, "当前保护账套尚未解锁");
        let key = [20_u8; 32];
        ensure_export_unlocked(&root, Some(&key)).expect("unlocked export");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn protected_failed_import_report_is_encrypted() {
        let root = std::env::temp_dir().join(format!(
            "erp-csv-failure-protected-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let failures = vec![(
            4_usize,
            vec!["bad".to_string()],
            "字段 price_cents 必须是整数".to_string(),
        )];
        let master = [18_u8; 32];
        let path = write_failed_import_file_with_master(
            &root,
            "IMP-PROTECTED",
            &["price_cents".to_string()],
            &failures,
            Some(&master),
        )
        .expect("write protected report")
        .expect("report path");
        assert!(path.ends_with(".csv.enc"));
        let encrypted = fs::read(&path).expect("read protected report");
        assert!(encrypted.starts_with(ENCRYPTED_MAGIC));
        let content = decrypt_bytes(&master, &encrypted).expect("decrypt protected report");
        let content = String::from_utf8(content).expect("utf8 report");
        assert!(content.contains("price_cents,line,error"));
        assert!(content.contains("bad,4,"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn csv_import_strategies_commit_or_rollback_the_entire_batch_as_requested() {
        let root =
            std::env::temp_dir().join(format!("erp-csv-strategy-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let source = b"code,base_resin\nCSV-IMPORT-GOOD,PP\nCSV-IMPORT-BAD,\n";

        let mut skip_conn = memory_db();
        let skip = import_csv_to_connection(
            &mut skip_conn,
            &root,
            "materials",
            "materials.csv",
            source,
            "skip",
        )
        .expect("skip import");
        assert_eq!(skip["inserted"], 1);
        assert_eq!(skip["failed"], 1);
        assert_eq!(skip["rolled_back"], false);
        let failed_file = skip["failed_file"].as_str().expect("failed file");
        assert!(Path::new(failed_file).exists());
        assert_eq!(
            skip_conn
                .query_row(
                    "SELECT COUNT(*) FROM materials WHERE code='CSV-IMPORT-GOOD'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .expect("skip count"),
            1
        );

        let mut rollback_conn = memory_db();
        let rollback = import_csv_to_connection(
            &mut rollback_conn,
            &root,
            "materials",
            "materials.csv",
            source,
            "rollback",
        )
        .expect("rollback import");
        assert_eq!(rollback["inserted"], 0);
        assert_eq!(rollback["failed"], 1);
        assert_eq!(rollback["rolled_back"], true);
        assert_eq!(
            rollback_conn
                .query_row(
                    "SELECT COUNT(*) FROM materials WHERE code='CSV-IMPORT-GOOD'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .expect("rollback count"),
            0
        );
        assert_eq!(
            rollback_conn
                .query_row("SELECT COUNT(*) FROM import_batches", [], |row| row
                    .get::<_, i64>(0))
                .expect("rollback batch"),
            1
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn attachments_are_copied_deduplicated_and_hash_checked() {
        let root = std::env::temp_dir().join(format!("erp-attachment-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        ensure_dirs(&root).expect("directories");
        let mut conn = memory_db();
        let now = now_iso();
        let customer_id = conn
            .execute(
                "INSERT INTO customers(name,created_at,updated_at) VALUES ('附件客户',?1,?1)",
                [&now],
            )
            .expect("customer") as i64;
        let result = save_attachment_to_connection(
            &mut conn,
            &root,
            None,
            "customers",
            customer_id,
            "报价\\报价单.pdf",
            b"attachment payload",
        )
        .expect("save attachment");
        let relative = result["relative_path"].as_str().expect("relative path");
        assert!(relative.starts_with("files/customers/"));
        assert!(relative.contains("/plain/"));
        assert_eq!(result["integrity"], "verified");
        assert_eq!(result["encrypted"], false);
        assert_eq!(result["file_name"], "报价单.pdf");
        assert!(root.join(relative).is_file());
        let duplicate = save_attachment_to_connection(
            &mut conn,
            &root,
            None,
            "customers",
            customer_id,
            "another-name.pdf",
            b"attachment payload",
        )
        .expect("deduplicate attachment");
        assert_eq!(duplicate["deduplicated"], true);
        assert_eq!(duplicate["id"], result["id"]);
        fs::write(root.join(relative), b"tampered").expect("tamper");
        let listed =
            list_attachments_conn(&conn, &root, None, Some("customers"), Some(customer_id))
                .expect("list attachments");
        assert_eq!(listed[0]["integrity"], "checksum_mismatch");
        let repaired = save_attachment_to_connection(
            &mut conn,
            &root,
            None,
            "customers",
            customer_id,
            "报价\\报价单.pdf",
            b"attachment payload",
        )
        .expect("repair attachment");
        assert_eq!(repaired["repaired"], true);
        assert_eq!(repaired["id"], result["id"]);
        assert_eq!(repaired["integrity"], "verified");
        let repaired_relative = repaired["relative_path"]
            .as_str()
            .expect("repaired relative path");
        assert_ne!(repaired_relative, relative);
        assert!(!root.join(relative).exists());
        assert!(root.join(repaired_relative).is_file());
        assert!(read_attachment_from_connection(
            &conn,
            &root,
            None,
            result["id"].as_i64().expect("id")
        )
        .is_ok());
        let plain_enc = save_attachment_to_connection(
            &mut conn,
            &root,
            None,
            "customers",
            customer_id,
            "普通文件.enc",
            b"plain enc suffix",
        )
        .expect("save plain enc attachment");
        assert_eq!(plain_enc["encrypted"], false);
        assert_eq!(plain_enc["integrity"], "verified");
        assert_eq!(plain_enc["file_name"], "普通文件.enc");
        let plain_enc_id = plain_enc["id"].as_i64().expect("plain enc id");
        let downloaded_plain_enc =
            read_attachment_from_connection(&conn, &root, None, plain_enc_id)
                .expect("read plain enc attachment");
        assert_eq!(downloaded_plain_enc["file_name"], "普通文件.enc");
        assert_eq!(
            downloaded_plain_enc["bytes"],
            json!([112, 108, 97, 105, 110, 32, 101, 110, 99, 32, 115, 117, 102, 102, 105, 120])
        );
        assert!(validate_relative_attachment_path("../escape").is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn enabling_protection_encrypts_existing_plain_attachments() {
        let root = std::env::temp_dir().join(format!(
            "erp-enable-protection-attachment-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        ensure_dirs(&root).expect("directories");
        let mut conn = memory_db();
        let now = now_iso();
        let customer_id = conn
            .execute(
                "INSERT INTO customers(name,created_at,updated_at) VALUES ('启用保护客户',?1,?1)",
                [&now],
            )
            .expect("customer") as i64;
        let original = save_attachment_to_connection(
            &mut conn,
            &root,
            None,
            "customers",
            customer_id,
            "资料.pdf",
            b"plain attachment",
        )
        .expect("plain attachment");
        let old_relative = original["relative_path"]
            .as_str()
            .expect("old relative path")
            .to_string();
        let master = [23_u8; 32];
        encrypt_attachments_for_enable(&mut conn, &root, &master).expect("encrypt attachments");
        let listed = list_attachments_conn(
            &conn,
            &root,
            Some(&master),
            Some("customers"),
            Some(customer_id),
        )
        .expect("list encrypted attachments");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0]["integrity"], "verified");
        assert_eq!(listed[0]["encrypted"], true);
        let encrypted_relative = listed[0]["relative_path"]
            .as_str()
            .expect("encrypted relative path");
        assert!(encrypted_relative.contains("/encrypted/"));
        assert!(!root.join(&old_relative).exists());
        assert!(root.join(encrypted_relative).is_file());
        let stored = fs::read(root.join(encrypted_relative)).expect("encrypted payload");
        assert!(stored.starts_with(ENCRYPTED_MAGIC));
        let downloaded = read_attachment_from_connection(
            &conn,
            &root,
            Some(&master),
            original["id"].as_i64().expect("attachment id"),
        )
        .expect("download encrypted attachment");
        assert_eq!(
            downloaded["bytes"],
            json!([112, 108, 97, 105, 110, 32, 97, 116, 116, 97, 99, 104, 109, 101, 110, 116])
        );
        encrypt_attachments_for_enable(&mut conn, &root, &master).expect("idempotent encryption");
        let listed_again = list_attachments_conn(
            &conn,
            &root,
            Some(&master),
            Some("customers"),
            Some(customer_id),
        )
        .expect("list after idempotent encryption");
        assert_eq!(listed_again.len(), 1);
        assert_eq!(listed_again[0]["relative_path"], encrypted_relative);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn disabling_protection_rejects_malformed_attachment_hash_without_panic() {
        let root = std::env::temp_dir().join(format!(
            "erp-malformed-attachment-hash-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        ensure_dirs(&root).expect("directories");
        let mut conn = memory_db();
        let now = now_iso();
        let customer_id = conn
            .execute(
                "INSERT INTO customers(name,created_at,updated_at) VALUES ('异常哈希客户',?1,?1)",
                [&now],
            )
            .expect("customer") as i64;
        let master = [19_u8; 32];
        let relative = format!("files/customers/{customer_id}/encrypted/20260913-120000.000-abcdef123456-corrupt.bin.enc");
        let payload = encrypt_bytes(&master, b"payload").expect("encrypt payload");
        write_attachment_payload(&root, &relative, &payload).expect("write payload");
        conn.execute("INSERT INTO attachments(object_type,object_id,relative_path,hash,created_at) VALUES ('customers',?1,?2,'bad-hash',?3)", params![customer_id, relative, now]).expect("attachment row");
        let error = decrypt_attachments_for_disable(&mut conn, &root, &master)
            .expect_err("malformed hash should fail");
        assert!(error.contains("哈希无效"));
        assert!(root.join(&relative).is_file());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn protected_attachments_round_trip_through_download_helper() {
        let root = std::env::temp_dir().join(format!(
            "erp-protected-attachment-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        ensure_dirs(&root).expect("directories");
        let mut conn = memory_db();
        let now = now_iso();
        let customer_id = conn
            .execute(
                "INSERT INTO customers(name,created_at,updated_at) VALUES ('加密附件客户',?1,?1)",
                [&now],
            )
            .expect("customer") as i64;
        let master = [11_u8; 32];
        let result = save_attachment_to_connection(
            &mut conn,
            &root,
            Some(&master),
            "customers",
            customer_id,
            "测试.enc",
            b"protected payload",
        )
        .expect("save protected attachment");
        let relative = result["relative_path"].as_str().expect("relative path");
        assert!(relative.contains("/encrypted/"));
        assert!(relative.ends_with(".enc"));
        assert!(fs::read(root.join(relative))
            .expect("stored payload")
            .starts_with(ENCRYPTED_MAGIC));
        let downloaded = read_attachment_from_connection(
            &conn,
            &root,
            Some(&master),
            result["id"].as_i64().expect("id"),
        )
        .expect("download attachment");
        assert_eq!(
            downloaded["bytes"],
            json!([
                112, 114, 111, 116, 101, 99, 116, 101, 100, 32, 112, 97, 121, 108, 111, 97, 100
            ])
        );
        assert_eq!(downloaded["file_name"], "测试.enc");
        let locked =
            list_attachments_conn(&conn, &root, None, Some("customers"), Some(customer_id))
                .expect("list locked attachment");
        assert_eq!(locked[0]["integrity"], "locked");
        decrypt_attachments_for_disable(&mut conn, &root, &master)
            .expect("decrypt attachments for disable");
        let unlocked =
            list_attachments_conn(&conn, &root, None, Some("customers"), Some(customer_id))
                .expect("list decrypted attachment");
        assert_eq!(unlocked[0]["integrity"], "verified");
        assert_eq!(unlocked[0]["encrypted"], false);
        assert!(unlocked[0]["relative_path"]
            .as_str()
            .unwrap_or_default()
            .contains("/plain/"));
        let reopened =
            read_attachment_from_connection(&conn, &root, None, result["id"].as_i64().expect("id"))
                .expect("read decrypted attachment");
        assert_eq!(reopened["file_name"], "测试.enc");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn file_operation_errors_explain_lock_and_missing_file_recovery() {
        let path = Path::new("export/locked.csv");
        let locked = std::io::Error::from_raw_os_error(32);
        let locked_message = file_operation_error(path, locked);
        assert!(locked_message.contains("占用程序") || locked_message.contains("无访问权限"));
        assert!(locked_message.contains("export/locked.csv"));

        let missing = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert!(file_operation_error(path, missing).contains("文件不存在"));
    }

    #[cfg(windows)]
    fn open_exclusive_for_test(path: &Path) -> fs::File {
        use std::fs::OpenOptions;
        use std::os::windows::fs::OpenOptionsExt;

        OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(path)
            .expect("open exclusive test handle")
    }

    #[cfg(windows)]
    #[test]
    fn locked_export_and_attachment_files_preserve_target_and_cleanup_temps() {
        let root = std::env::temp_dir().join(format!(
            "erp-file-lock-test-{}-{}",
            std::process::id(),
            hex::encode(random_bytes::<4>())
        ));
        let _ = fs::remove_dir_all(&root);
        ensure_dirs(&root).expect("directories");

        let export_path = root.join("export").join("locked.csv");
        write_bytes_atomically(&export_path, b"old export").expect("seed export");
        let old_export = fs::read(&export_path).expect("read seeded export");
        let export_lock = open_exclusive_for_test(&export_path);
        let export_error = write_bytes_atomically(&export_path, b"new export")
            .expect_err("locked export must fail");
        assert!(export_error.contains("占用程序") || export_error.contains("无访问权限"));
        assert!(!fs::read_dir(export_path.parent().expect("export parent"))
            .expect("export directory")
            .flatten()
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with(".locked.csv.")
                && entry.file_name().to_string_lossy().ends_with(".tmp")));
        drop(export_lock);
        assert_eq!(fs::read(&export_path).expect("old export"), old_export);
        write_bytes_atomically(&export_path, b"new export").expect("retry export");
        assert_eq!(fs::read(&export_path).expect("new export"), b"new export");

        let relative = "files/customers/1/plain/locked.txt";
        write_attachment_payload(&root, relative, b"old attachment").expect("seed attachment");
        let attachment_path = root.join(relative);
        let old_attachment = fs::read(&attachment_path).expect("read seeded attachment");
        let attachment_lock = open_exclusive_for_test(&attachment_path);
        let attachment_error = write_attachment_payload(&root, relative, b"new attachment")
            .expect_err("locked attachment must fail");
        assert!(attachment_error.contains("占用程序") || attachment_error.contains("无访问权限"));
        assert!(
            !fs::read_dir(attachment_path.parent().expect("attachment parent"))
                .expect("attachment directory")
                .flatten()
                .any(|entry| entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".locked.txt.")
                    && entry.file_name().to_string_lossy().ends_with(".tmp"))
        );
        drop(attachment_lock);
        assert_eq!(
            fs::read(&attachment_path).expect("old attachment"),
            old_attachment
        );
        write_attachment_payload(&root, relative, b"new attachment").expect("retry attachment");
        assert_eq!(
            fs::read(&attachment_path).expect("new attachment"),
            b"new attachment"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn startup_recovery_marks_existing_data_and_preserves_clean_state_for_bad_passwords() {
        let root =
            std::env::temp_dir().join(format!("erp-startup-recovery-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        ensure_dirs(&root).expect("directories");

        let first_run = read_previous_exit(&root);
        assert!(!first_run.dirty);
        assert!(first_run.time.is_none());

        fs::write(active_db_path(&root), b"existing database marker").expect("existing data");
        let missing_state = read_previous_exit(&root);
        assert!(missing_state.dirty);
        assert!(missing_state.time.is_none());

        write_exit_state(&root, "clean", None).expect("clean exit state");
        let clean_state = read_previous_exit(&root);
        assert!(!clean_state.dirty);
        assert!(clean_state.time.is_some());

        let password = "correct-local-password";
        let salt = [1_u8; 16];
        let master = [2_u8; 32];
        let password_key = derive_password_key(password, &salt).expect("password key");
        let recovery_key = sha_key("recovery-key");
        let config = ProtectionConfig {
            version: 1,
            salt_b64: B64.encode(salt),
            password_wrapped_b64: wrap_master(&password_key, &master).expect("password wrap"),
            recovery_wrapped_b64: wrap_master(&recovery_key, &master).expect("recovery wrap"),
        };
        save_protection_config(&root, &config).expect("protection config");

        assert!(mark_startup_in_progress(&root, Some("wrong-password")).is_err());
        assert!(
            !read_previous_exit(&root).dirty,
            "a rejected password must not create a false dirty exit"
        );

        let (previous_exit, unlocked_master) =
            mark_startup_in_progress(&root, Some(password)).expect("startup mark");
        assert!(!previous_exit.dirty);
        assert_eq!(unlocked_master, Some(master));
        assert!(read_previous_exit(&root).dirty);

        append_runtime_log(
            &root,
            "unclean_exit_recovered",
            json!({ "integrity": "verified" }),
        )
        .expect("recovery log");
        let log_path = root
            .join("logs")
            .join(format!("erp-{}.log", Local::now().format("%Y%m%d")));
        let log = fs::read_to_string(log_path).expect("read recovery log");
        assert!(log.contains("unclean_exit_recovered"));
        assert!(!log.contains(password));
        let _ = fs::remove_dir_all(&root);
    }
}
