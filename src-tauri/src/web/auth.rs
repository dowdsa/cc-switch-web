use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use argon2::password_hash::{
    PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use base64::Engine as _;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const SESSION_TTL_SECS: i64 = 30 * 24 * 60 * 60;
const MAX_FAILURES: u32 = 5;
const LOCKOUT: Duration = Duration::from_secs(15 * 60);
const MIN_PASSWORD_LEN: usize = 8;
const MAX_USERNAME_LEN: usize = 64;

#[derive(Debug)]
pub enum AuthError {
    Unauthorized,
    BadRequest(String),
    Conflict(String),
    Locked(u64),
    Internal(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::Unauthorized => write!(f, "未登录或会话已过期 (Unauthorized)"),
            AuthError::BadRequest(m) => write!(f, "{m}"),
            AuthError::Conflict(m) => write!(f, "{m}"),
            AuthError::Locked(secs) => write!(f, "登录尝试过于频繁，请在 {secs} 秒后重试"),
            AuthError::Internal(m) => write!(f, "认证服务内部错误: {m}"),
        }
    }
}

impl std::error::Error for AuthError {}

struct Attempt {
    failures: u32,
    locked_until: Option<Instant>,
}

pub struct AuthStore {
    conn: Mutex<Connection>,
    setup_token_path: PathBuf,
    setup_token: Mutex<Option<String>>,
    attempts: Mutex<HashMap<String, Attempt>>,
    dummy_hash: String,
}

pub struct AuthStatus {
    pub needs_setup: bool,
    pub authenticated: bool,
    pub username: Option<String>,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn random_bytes(len: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(len);
    while bytes.len() < len {
        bytes.extend_from_slice(Uuid::new_v4().as_bytes());
    }
    bytes.truncate(len);
    bytes
}

fn new_session_token() -> String {
    b64(&random_bytes(32))
}

fn hash_password_blocking(password: &str) -> Result<String, AuthError> {
    let salt_bytes = random_bytes(16);
    let salt = SaltString::encode_b64(&salt_bytes)
        .map_err(|e| AuthError::Internal(format!("salt encode: {e}")))?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AuthError::Internal(format!("password hash: {e}")))
}

fn verify_password_blocking(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

fn validate_username(username: &str) -> Result<&str, AuthError> {
    let name = username.trim();
    if name.is_empty() {
        return Err(AuthError::BadRequest("用户名不能为空".into()));
    }
    if name.chars().count() > MAX_USERNAME_LEN {
        return Err(AuthError::BadRequest("用户名过长".into()));
    }
    if name.chars().any(|c| c.is_control()) {
        return Err(AuthError::BadRequest("用户名包含非法字符".into()));
    }
    Ok(name)
}

fn validate_password(password: &str) -> Result<(), AuthError> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(AuthError::BadRequest(format!(
            "密码至少需要 {MIN_PASSWORD_LEN} 个字符"
        )));
    }
    Ok(())
}

impl AuthStore {
    pub fn init(dir: &Path) -> Result<Self, AuthError> {
        std::fs::create_dir_all(dir)
            .map_err(|e| AuthError::Internal(format!("create auth dir: {e}")))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }

        let db_path = dir.join("auth.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| AuthError::Internal(format!("open auth db: {e}")))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o600));
        }

        conn.busy_timeout(Duration::from_secs(5))
            .map_err(|e| AuthError::Internal(e.to_string()))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS web_user (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                username TEXT UNIQUE NOT NULL,
                password_hash TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                password_changed_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS web_session (
                token_hash TEXT PRIMARY KEY,
                user_id INTEGER NOT NULL REFERENCES web_user(id) ON DELETE CASCADE,
                created_at INTEGER NOT NULL,
                last_seen_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL,
                ua TEXT NOT NULL DEFAULT '',
                ip TEXT NOT NULL DEFAULT ''
             );
             CREATE TABLE IF NOT EXISTS web_audit (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                at INTEGER NOT NULL,
                ip TEXT NOT NULL DEFAULT '',
                event TEXT NOT NULL
             );",
        )
        .map_err(|e| AuthError::Internal(format!("init auth schema: {e}")))?;

        let dummy_hash = hash_password_blocking("cc-switch-web-dummy-password")?;

        Ok(Self {
            conn: Mutex::new(conn),
            setup_token_path: dir.join("setup-token"),
            setup_token: Mutex::new(None),
            attempts: Mutex::new(HashMap::new()),
            dummy_hash,
        })
    }

    fn has_user_locked(conn: &Connection) -> Result<bool, AuthError> {
        conn.query_row("SELECT COUNT(*) FROM web_user", [], |row| {
            row.get::<_, i64>(0)
        })
        .map(|count| count > 0)
        .map_err(|e| AuthError::Internal(e.to_string()))
    }

    pub fn needs_setup(&self) -> bool {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        matches!(Self::has_user_locked(&conn), Ok(false))
    }

    pub fn username(&self) -> Option<String> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.query_row("SELECT username FROM web_user WHERE id = 1", [], |row| {
            row.get(0)
        })
        .optional()
        .ok()
        .flatten()
    }

    pub fn status(&self, raw_token: Option<&str>) -> AuthStatus {
        let username = self.username();
        let authenticated = raw_token
            .and_then(|token| self.validate_session(token))
            .is_some();
        AuthStatus {
            needs_setup: username.is_none(),
            authenticated,
            username,
        }
    }

    /// 首次启动时下发一次性 setup token（写入 0600 文件），账号创建后失效。
    pub fn ensure_setup_token(&self) -> Option<String> {
        if !self.needs_setup() {
            return None;
        }
        let mut guard = self.setup_token.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(token) = guard.as_ref() {
            return Some(token.clone());
        }
        if let Ok(env_token) = std::env::var("CC_SWITCH_WEB_SETUP_TOKEN") {
            let token = env_token.trim().to_string();
            if !token.is_empty() {
                *guard = Some(token.clone());
                return Some(token);
            }
        }
        if let Ok(existing) = std::fs::read_to_string(&self.setup_token_path) {
            let token = existing.trim().to_string();
            if !token.is_empty() {
                *guard = Some(token.clone());
                return Some(token);
            }
        }
        let token = b64(&random_bytes(24));
        if let Err(e) = write_private_file(&self.setup_token_path, token.as_bytes()) {
            log::error!("写入 setup token 失败: {e}");
        }
        *guard = Some(token.clone());
        Some(token)
    }

    fn verify_setup_token(&self, presented: &str) -> Result<(), AuthError> {
        let guard = self.setup_token.lock().unwrap_or_else(|e| e.into_inner());
        let Some(expected) = guard.as_ref() else {
            return Err(AuthError::BadRequest(
                "当前没有待完成的初始化（账号已存在或服务未生成 setup token）".into(),
            ));
        };
        if !constant_time_eq(expected.as_bytes(), presented.trim().as_bytes()) {
            return Err(AuthError::Unauthorized);
        }
        Ok(())
    }

    fn issue_session(
        &self,
        conn: &Connection,
        ua: &str,
        ip: &str,
    ) -> Result<String, AuthError> {
        let token = new_session_token();
        let now = now_secs();
        conn.execute(
            "INSERT INTO web_session (token_hash, user_id, created_at, last_seen_at, expires_at, ua, ip)
             VALUES (?1, 1, ?2, ?2, ?3, ?4, ?5)",
            params![
                hash_token(&token),
                now,
                now + SESSION_TTL_SECS,
                truncate(ua, 300),
                truncate(ip, 100)
            ],
        )
        .map_err(|e| AuthError::Internal(e.to_string()))?;
        Ok(token)
    }

    fn audit(&self, ip: &str, event: &str) {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let _ = conn.execute(
            "INSERT INTO web_audit (at, ip, event) VALUES (?1, ?2, ?3)",
            params![now_secs(), truncate(ip, 100), truncate(event, 200)],
        );
    }

    pub async fn setup(
        &self,
        setup_token: &str,
        username: &str,
        password: &str,
        ip: &str,
        ua: &str,
    ) -> Result<String, AuthError> {
        self.verify_setup_token(setup_token)?;
        let username = validate_username(username)?.to_string();
        validate_password(password)?;
        if !self.needs_setup() {
            return Err(AuthError::Conflict("管理员账号已存在".into()));
        }

        let hash = tokio::task::spawn_blocking({
            let password = password.to_string();
            move || hash_password_blocking(&password)
        })
        .await
        .map_err(|e| AuthError::Internal(e.to_string()))??;

        let token = {
            let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
            if Self::has_user_locked(&conn)? {
                return Err(AuthError::Conflict("管理员账号已存在".into()));
            }
            let now = now_secs();
            conn.execute(
                "INSERT INTO web_user (id, username, password_hash, created_at, updated_at, password_changed_at)
                 VALUES (1, ?1, ?2, ?3, ?3, ?3)",
                params![username, hash, now],
            )
            .map_err(|e| AuthError::Internal(e.to_string()))?;
            let token = self.issue_session(&conn, ua, ip)?;
            token
        };

        let _ = std::fs::remove_file(&self.setup_token_path);
        *self.setup_token.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.audit(ip, "setup_completed");
        Ok(token)
    }

    fn rate_key(ip: &str, username: &str) -> String {
        format!("{ip}|{}", username.trim().to_lowercase())
    }

    fn check_rate(&self, key: &str) -> Result<(), AuthError> {
        let mut map = self.attempts.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(attempt) = map.get_mut(key) {
            if let Some(until) = attempt.locked_until {
                if until > Instant::now() {
                    return Err(AuthError::Locked(
                        until.saturating_duration_since(Instant::now()).as_secs() + 1,
                    ));
                }
                attempt.locked_until = None;
                attempt.failures = 0;
            }
        }
        Ok(())
    }

    fn record_failure(&self, key: &str) {
        let mut map = self.attempts.lock().unwrap_or_else(|e| e.into_inner());
        let entry = map.entry(key.to_string()).or_insert(Attempt {
            failures: 0,
            locked_until: None,
        });
        entry.failures += 1;
        if entry.failures >= MAX_FAILURES {
            entry.locked_until = Some(Instant::now() + LOCKOUT);
        }
    }

    fn clear_failures(&self, key: &str) {
        let mut map = self.attempts.lock().unwrap_or_else(|e| e.into_inner());
        map.remove(key);
    }

    pub async fn login(
        &self,
        username: &str,
        password: &str,
        ip: &str,
        ua: &str,
    ) -> Result<String, AuthError> {
        let key = Self::rate_key(ip, username);
        self.check_rate(&key)?;

        let row: Option<(String, String)> = {
            let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
            conn.query_row(
                "SELECT username, password_hash FROM web_user WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|e| AuthError::Internal(e.to_string()))?
        };

        let (name, hash) = match &row {
            Some((name, hash)) if name == username.trim() => (name.clone(), hash.clone()),
            _ => {
                let dummy = self.dummy_hash.clone();
                let supplied = password.to_string();
                let _ = tokio::task::spawn_blocking(move || {
                    verify_password_blocking(&supplied, &dummy)
                })
                .await;
                self.record_failure(&key);
                self.audit(ip, "login_failed");
                return Err(AuthError::Unauthorized);
            }
        };

        let supplied = password.to_string();
        let verified = tokio::task::spawn_blocking(move || {
            verify_password_blocking(&supplied, &hash)
        })
        .await
        .map_err(|e| AuthError::Internal(e.to_string()))?;

        if !verified {
            self.record_failure(&key);
            self.audit(ip, "login_failed");
            return Err(AuthError::Unauthorized);
        }

        self.clear_failures(&key);
        let token = {
            let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
            conn.execute(
                "DELETE FROM web_session WHERE expires_at <= ?1",
                params![now_secs()],
            )
            .ok();
            self.issue_session(&conn, ua, ip)?
        };
        self.audit(ip, &format!("login_ok:{name}"));
        Ok(token)
    }

    pub fn validate_session(&self, raw_token: &str) -> Option<String> {
        if raw_token.is_empty() {
            return None;
        }
        let token_hash = hash_token(raw_token);
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let row: Option<(String, i64)> = conn
            .query_row(
                "SELECT u.username, s.expires_at
                 FROM web_session s JOIN web_user u ON u.id = s.user_id
                 WHERE s.token_hash = ?1",
                params![token_hash],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .ok()?;
        let (username, expires_at) = row?;
        let now = now_secs();
        if expires_at <= now {
            let _ = conn.execute(
                "DELETE FROM web_session WHERE token_hash = ?1",
                params![token_hash],
            );
            return None;
        }
        let _ = conn.execute(
            "UPDATE web_session SET last_seen_at = ?2, expires_at = ?3 WHERE token_hash = ?1",
            params![token_hash, now, now + SESSION_TTL_SECS],
        );
        Some(username)
    }

    pub fn logout(&self, raw_token: &str) {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let _ = conn.execute(
            "DELETE FROM web_session WHERE token_hash = ?1",
            params![hash_token(raw_token)],
        );
    }

    pub async fn change_password(
        &self,
        raw_token: &str,
        current_password: &str,
        new_password: &str,
        ip: &str,
        ua: &str,
    ) -> Result<String, AuthError> {
        let username = self
            .validate_session(raw_token)
            .ok_or(AuthError::Unauthorized)?;
        validate_password(new_password)?;

        let hash: String = {
            let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
            conn.query_row(
                "SELECT password_hash FROM web_user WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|e| AuthError::Internal(e.to_string()))?
        };

        let supplied = current_password.to_string();
        let ok = tokio::task::spawn_blocking(move || {
            verify_password_blocking(&supplied, &hash)
        })
        .await
        .map_err(|e| AuthError::Internal(e.to_string()))?;
        if !ok {
            self.audit(ip, "change_password_failed");
            return Err(AuthError::Unauthorized);
        }

        let new_hash = tokio::task::spawn_blocking({
            let new_password = new_password.to_string();
            move || hash_password_blocking(&new_password)
        })
        .await
        .map_err(|e| AuthError::Internal(e.to_string()))??;

        let token = {
            let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
            conn.execute(
                "UPDATE web_user SET password_hash = ?1, updated_at = ?2, password_changed_at = ?2
                 WHERE id = 1",
                params![new_hash, now_secs()],
            )
            .map_err(|e| AuthError::Internal(e.to_string()))?;
            // 修改密码后让所有旧会话（包括当前）失效，再签发新会话。
            conn.execute("DELETE FROM web_session", [])
                .map_err(|e| AuthError::Internal(e.to_string()))?;
            self.issue_session(&conn, ua, ip)?
        };
        self.audit(ip, &format!("password_changed:{username}"));
        Ok(token)
    }

    pub async fn reset_password(&self, new_password: &str) -> Result<String, AuthError> {
        validate_password(new_password)?;
        let username = self
            .username()
            .ok_or_else(|| AuthError::BadRequest("尚未创建管理员账号".into()))?;
        let new_hash = tokio::task::spawn_blocking({
            let new_password = new_password.to_string();
            move || hash_password_blocking(&new_password)
        })
        .await
        .map_err(|e| AuthError::Internal(e.to_string()))??;
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "UPDATE web_user SET password_hash = ?1, updated_at = ?2, password_changed_at = ?2
             WHERE id = 1",
            params![new_hash, now_secs()],
        )
        .map_err(|e| AuthError::Internal(e.to_string()))?;
        conn.execute("DELETE FROM web_session", [])
            .map_err(|e| AuthError::Internal(e.to_string()))?;
        Ok(username)
    }
}

fn truncate(input: &str, max: usize) -> String {
    input.chars().take(max).collect()
}

fn write_private_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(0o600);
        let mut file = options.open(path)?;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        file.write_all(bytes)
    }
    #[cfg(not(unix))]
    {
        let mut file = options.open(path)?;
        file.write_all(bytes)
    }
}
