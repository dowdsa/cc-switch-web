use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Extension, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;
use tower_http::services::{ServeDir, ServeFile};

use crate::web::auth::AuthError;
use crate::web::rpc::{self, WebCtx};
use crate::web::{CookieSecure, WebOptions};

const SESSION_COOKIE: &str = "ccsw_session";
const SESSION_MAX_AGE_SECS: i64 = 30 * 24 * 60 * 60;

#[derive(Clone)]
struct HttpState {
    ctx: Arc<WebCtx>,
    options: Arc<WebOptions>,
    csp: Option<String>,
}

#[derive(Clone)]
struct AuthedUser {
    #[allow(dead_code)]
    username: String,
    token: String,
}

pub async fn serve(ctx: Arc<WebCtx>, options: WebOptions) -> Result<(), String> {
    let index_path = options.dist_dir.join("index.html");
    if !index_path.exists() {
        log::warn!(
            "前端产物不存在: {}（请先执行 pnpm build:web）",
            index_path.display()
        );
    }
    let csp = build_csp(&index_path);
    let state = Arc::new(HttpState {
        ctx,
        options: Arc::new(options),
        csp,
    });

    let protected = Router::new()
        .route("/auth/me", get(me))
        .route("/auth/logout", post(logout))
        .route("/auth/change-password", post(change_password))
        .route("/rpc", post(rpc_handler))
        .route("/rpc/stream", post(rpc_stream))
        .route("/events", get(sse))
        .route(
            "/upload",
            post(upload_handler).layer(DefaultBodyLimit::max(64 * 1024 * 1024)),
        )
        .route("/export/sql", get(export_sql))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_auth,
        ));

    let public = Router::new()
        .route("/health", get(health))
        .route("/meta", get(meta))
        .route("/auth/status", get(auth_status))
        .route("/auth/setup", post(setup))
        .route("/auth/login", post(login));

    let api = public
        .merge(protected)
        .fallback(api_not_found)
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024));

    let index = state.options.dist_dir.join("index.html");
    let static_files = ServeDir::new(&state.options.dist_dir)
        .not_found_service(ServeFile::new(index));

    let app = Router::new()
        .nest("/api", api)
        .fallback_service(static_files)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            security_headers,
        ))
        .with_state(state.clone());

    let host = state.options.host.clone();
    let port = state.options.port;
    let listener = tokio::net::TcpListener::bind((host.as_str(), port))
        .await
        .map_err(|e| format!("监听 {host}:{port} 失败: {e}"))?;
    let bound = listener
        .local_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| format!("{host}:{port}"));
    println!("[cc-switch-web] 已启动: http://{bound}");
    println!(
        "[cc-switch-web] 静态资源: {}",
        state.options.dist_dir.display()
    );

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .map_err(|e| e.to_string())
}

async fn security_headers(
    State(state): State<Arc<HttpState>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("Referrer-Policy", HeaderValue::from_static("no-referrer"));
    headers.insert(
        "X-Frame-Options",
        HeaderValue::from_static("DENY"),
    );
    if let Some(csp) = &state.csp {
        if let Ok(value) = HeaderValue::from_str(csp) {
            headers.insert("Content-Security-Policy", value);
        }
    }
    response
}

async fn require_auth(
    State(state): State<Arc<HttpState>>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let method = request.method().clone();
    if method != Method::GET && method != Method::HEAD && method != Method::OPTIONS {
        if let Err(response) = check_csrf(&state, request.headers()) {
            return response;
        }
    }

    let Some(token) = cookie_value(request.headers(), SESSION_COOKIE) else {
        return unauthorized();
    };
    let Some(username) = state.ctx.auth.validate_session(&token) else {
        return unauthorized();
    };
    request
        .extensions_mut()
        .insert(AuthedUser { username, token });
    next.run(request).await
}

fn check_csrf(state: &HttpState, headers: &HeaderMap) -> Result<(), Response> {
    let header_ok = headers
        .get("x-cc-switch-web")
        .and_then(|value| value.to_str().ok())
        .map(|value| value == "1")
        .unwrap_or(false);
    if !header_ok {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "csrf",
            "缺少 X-CC-Switch-Web 请求头",
        ));
    }

    if let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    {
        if !origin_allowed(state, headers, origin) {
            return Err(json_error(
                StatusCode::FORBIDDEN,
                "origin",
                "请求来源与主机不匹配",
            ));
        }
    }
    Ok(())
}

fn origin_allowed(state: &HttpState, headers: &HeaderMap, origin: &str) -> bool {
    if state
        .options
        .allowed_origins
        .iter()
        .any(|allowed| allowed == origin)
    {
        return true;
    }
    let Ok(url) = url::Url::parse(origin) else {
        return false;
    };
    let Some(request_host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let request_hostname = request_host
        .rsplit_once(':')
        .filter(|(_, port)| port.parse::<u16>().is_ok())
        .map(|(host, _)| host)
        .unwrap_or(request_host);
    url.host_str()
        .map(|origin_host| origin_host.eq_ignore_ascii_case(request_hostname))
        .unwrap_or(false)
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in raw.split(';') {
        if let Some((key, value)) = part.trim().split_once('=') {
            if key.trim() == name {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

fn secure_cookie(state: &HttpState, headers: &HeaderMap) -> bool {
    match state.options.cookie_secure {
        CookieSecure::Always => true,
        CookieSecure::Never => false,
        CookieSecure::Auto => headers
            .get("x-forwarded-proto")
            .and_then(|value| value.to_str().ok())
            .map(|proto| proto.eq_ignore_ascii_case("https"))
            .unwrap_or(false),
    }
}

fn session_cookie(state: &HttpState, headers: &HeaderMap, token: &str) -> String {
    let mut cookie = format!(
        "{SESSION_COOKIE}={token}; HttpOnly; Path=/; SameSite=Strict; Max-Age={SESSION_MAX_AGE_SECS}"
    );
    if secure_cookie(state, headers) {
        cookie.push_str("; Secure");
    }
    cookie
}

fn expired_cookie(state: &HttpState, headers: &HeaderMap) -> String {
    let mut cookie =
        format!("{SESSION_COOKIE}=; HttpOnly; Path=/; SameSite=Strict; Max-Age=0");
    if secure_cookie(state, headers) {
        cookie.push_str("; Secure");
    }
    cookie
}

fn json_error(status: StatusCode, code: &str, message: impl Into<String>) -> Response {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message.into() } })),
    )
        .into_response()
}

fn unauthorized() -> Response {
    json_error(
        StatusCode::UNAUTHORIZED,
        "unauthorized",
        "未登录或会话已失效",
    )
}

fn auth_error(error: AuthError) -> Response {
    match error {
        AuthError::Unauthorized => json_error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "用户名或密码错误",
        ),
        AuthError::BadRequest(message) => {
            json_error(StatusCode::BAD_REQUEST, "bad_request", message)
        }
        AuthError::Conflict(message) => {
            json_error(StatusCode::CONFLICT, "conflict", message)
        }
        AuthError::Locked(seconds) => {
            let mut response = json_error(
                StatusCode::TOO_MANY_REQUESTS,
                "locked",
                format!("尝试次数过多，请 {seconds} 秒后重试"),
            );
            if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
                response.headers_mut().insert("Retry-After", value);
            }
            response
        }
        AuthError::Internal(message) => {
            log::error!("认证内部错误: {message}");
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "认证服务内部错误",
            )
        }
    }
}

fn json_with_cookie(value: Value, cookie: String) -> Response {
    let mut response = Json(value).into_response();
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

fn client_ip(headers: &HeaderMap, addr: SocketAddr) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| addr.ip().to_string())
}

fn user_agent(headers: &HeaderMap) -> String {
    headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

async fn health() -> Response {
    Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "mode": "web",
    }))
    .into_response()
}

async fn auth_status(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
) -> Response {
    let token = cookie_value(&headers, SESSION_COOKIE);
    let status = state.ctx.auth.status(token.as_deref());
    Json(json!({
        "needsSetup": status.needs_setup,
        "authenticated": status.authenticated,
        "username": if status.authenticated { status.username } else { None },
    }))
    .into_response()
}

#[derive(Deserialize)]
struct SetupBody {
    token: String,
    username: String,
    password: String,
}

async fn setup(
    State(state): State<Arc<HttpState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<SetupBody>,
) -> Response {
    if let Err(response) = check_csrf(&state, &headers) {
        return response;
    }
    let ip = client_ip(&headers, addr);
    match state
        .ctx
        .auth
        .setup(
            &body.token,
            &body.username,
            &body.password,
            &ip,
            &user_agent(&headers),
        )
        .await
    {
        Ok(token) => json_with_cookie(
            json!({ "authenticated": true, "username": body.username.trim() }),
            session_cookie(&state, &headers, &token),
        ),
        Err(AuthError::Unauthorized) => json_error(
            StatusCode::UNAUTHORIZED,
            "bad_setup_token",
            "初始化令牌无效",
        ),
        Err(error) => auth_error(error),
    }
}

#[derive(Deserialize)]
struct LoginBody {
    username: String,
    password: String,
}

async fn login(
    State(state): State<Arc<HttpState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<LoginBody>,
) -> Response {
    if let Err(response) = check_csrf(&state, &headers) {
        return response;
    }
    let ip = client_ip(&headers, addr);
    match state
        .ctx
        .auth
        .login(
            &body.username,
            &body.password,
            &ip,
            &user_agent(&headers),
        )
        .await
    {
        Ok(token) => json_with_cookie(
            json!({ "authenticated": true, "username": body.username.trim() }),
            session_cookie(&state, &headers, &token),
        ),
        Err(error) => auth_error(error),
    }
}

async fn logout(
    State(state): State<Arc<HttpState>>,
    Extension(user): Extension<AuthedUser>,
    headers: HeaderMap,
) -> Response {
    state.ctx.auth.logout(&user.token);
    let mut response = Json(json!({ "authenticated": false })).into_response();
    if let Ok(value) = HeaderValue::from_str(&expired_cookie(&state, &headers)) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

async fn me(Extension(user): Extension<AuthedUser>) -> Response {
    Json(json!({ "username": user.username })).into_response()
}

async fn meta(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
) -> Response {
    let authenticated = cookie_value(&headers, SESSION_COOKIE)
        .and_then(|token| state.ctx.auth.validate_session(&token))
        .is_some();
    let mut payload = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "platform": std::env::consts::OS,
        "mode": "web",
    });
    if authenticated {
        payload["homeDir"] =
            json!(crate::config::get_home_dir().display().to_string());
        payload["configDir"] = json!(state.ctx.config_dir.display().to_string());
    }
    Json(payload).into_response()
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
struct ChangePasswordBody {
    currentPassword: String,
    newPassword: String,
}

async fn change_password(
    State(state): State<Arc<HttpState>>,
    Extension(user): Extension<AuthedUser>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<ChangePasswordBody>,
) -> Response {
    let ip = client_ip(&headers, addr);
    match state
        .ctx
        .auth
        .change_password(
            &user.token,
            &body.currentPassword,
            &body.newPassword,
            &ip,
            &user_agent(&headers),
        )
        .await
    {
        Ok(token) => json_with_cookie(
            json!({ "ok": true }),
            session_cookie(&state, &headers, &token),
        ),
        Err(AuthError::Unauthorized) => json_error(
            StatusCode::BAD_REQUEST,
            "wrong_password",
            "当前密码错误",
        ),
        Err(error) => auth_error(error),
    }
}

#[derive(Deserialize)]
struct RpcBody {
    cmd: String,
    #[serde(default)]
    args: Value,
}

async fn rpc_handler(
    State(state): State<Arc<HttpState>>,
    Json(body): Json<RpcBody>,
) -> Response {
    if body.cmd == "get_session_image" {
        return session_image(&state, body.args).await;
    }
    match rpc::dispatch(&state.ctx, &body.cmd, body.args).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => {
            let (status, code, message) = error.parts();
            log::debug!("rpc `{}` 失败: {message}", body.cmd);
            json_error(status, code, message)
        }
    }
}

async fn session_image(_state: &HttpState, args: Value) -> Response {
    use crate::session_manager::model::ImageRef;

    let parsed = (|| -> Result<(String, String, ImageRef), Response> {
        let provider_id = args
            .get("providerId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                json_error(StatusCode::BAD_REQUEST, "bad_request", "缺少 providerId")
            })?
            .to_string();
        let source_path = args
            .get("sourcePath")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                json_error(StatusCode::BAD_REQUEST, "bad_request", "缺少 sourcePath")
            })?
            .to_string();
        let image: ImageRef = serde_json::from_value(
            args.get("image").cloned().unwrap_or(Value::Null),
        )
        .map_err(|e| {
            json_error(
                StatusCode::BAD_REQUEST,
                "bad_request",
                format!("image 参数格式错误: {e}"),
            )
        })?;
        Ok((provider_id, source_path, image))
    })();
    let (provider_id, source_path, image) = match parsed {
        Ok(value) => value,
        Err(response) => return response,
    };

    let result = tokio::task::spawn_blocking(move || {
        let source =
            crate::session_manager::content::validate_source(&provider_id, &source_path)?;
        crate::session_manager::content::load_image(&source, &image)
    })
    .await;

    match result {
        Ok(Ok(bytes)) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/octet-stream")],
            bytes,
        )
            .into_response(),
        Ok(Err(message)) => json_error(StatusCode::BAD_REQUEST, "bad_request", message),
        Err(error) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            format!("读取会话图片失败: {error}"),
        ),
    }
}

#[derive(Deserialize)]
struct RpcStreamBody {
    cmd: String,
    #[serde(default)]
    args: Value,
}

async fn rpc_stream(
    State(state): State<Arc<HttpState>>,
    Json(body): Json<RpcStreamBody>,
) -> Response {
    if body.cmd != "stream_session_messages" {
        return json_error(
            StatusCode::NOT_IMPLEMENTED,
            "unsupported",
            format!("流式命令 `{}` 尚未在 Web 模式中实现", body.cmd),
        );
    }
    let _ = &state;
    let provider_id = match body.args.get("providerId").and_then(Value::as_str) {
        Some(value) => value.to_string(),
        None => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "bad_request",
                "缺少 providerId",
            )
        }
    };
    let source_path = match body.args.get("sourcePath").and_then(Value::as_str) {
        Some(value) => value.to_string(),
        None => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "bad_request",
                "缺少 sourcePath",
            )
        }
    };

    let loaded = tokio::task::spawn_blocking(move || {
        crate::session_manager::load_transcript(&provider_id, &source_path)
    })
    .await;

    let stream = async_stream::stream! {
        match loaded {
            Ok(Ok(loaded)) => {
                use crate::session_manager::cache::chunk_ranges;
                use crate::session_manager::model::TranscriptChunk;

                let transcript = loaded.transcript;
                let header = TranscriptChunk::Header {
                    total: transcript.messages.len(),
                    turns: transcript.turns.clone(),
                    cached: loaded.cached,
                    parse_ms: loaded.parse_ms,
                };
                yield Ok::<String, Infallible>(format!(
                    "{}\n",
                    json!({ "type": "message", "message": header })
                ));
                for (start, end) in chunk_ranges(&transcript.message_bytes) {
                    let chunk = TranscriptChunk::Messages {
                        start,
                        messages: transcript.messages[start..end].to_vec(),
                    };
                    yield Ok(format!(
                        "{}\n",
                        json!({ "type": "message", "message": chunk })
                    ));
                }
                let done = TranscriptChunk::Done {
                    payload_bytes: transcript.approx_bytes as u64,
                };
                yield Ok(format!(
                    "{}\n",
                    json!({ "type": "done", "result": null, "message": done })
                ));
            }
            Ok(Err(message)) => {
                yield Ok(format!(
                    "{}\n",
                    json!({ "type": "error", "error": message })
                ));
            }
            Err(error) => {
                yield Ok(format!(
                    "{}\n",
                    json!({ "type": "error", "error": format!("读取会话失败: {error}") })
                ));
            }
        }
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "构建流式响应失败",
            )
        })
}

async fn api_not_found() -> Response {
    json_error(StatusCode::NOT_FOUND, "not_found", "接口不存在")
}

async fn export_sql(State(state): State<Arc<HttpState>>) -> Response {
    let db = state.ctx.state.db.clone();
    let result = tokio::task::spawn_blocking(move || db.export_sql_string()).await;
    match result {
        Ok(Ok(sql)) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/sql"),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"cc-switch-backup.sql\"",
                ),
            ],
            sql,
        )
            .into_response(),
        Ok(Err(error)) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            error.to_string(),
        ),
        Err(error) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            format!("导出失败: {error}"),
        ),
    }
}

async fn upload_handler(
    State(state): State<Arc<HttpState>>,
    mut multipart: axum::extract::Multipart,
) -> Response {
    let _ = &state;
    let dir = std::env::temp_dir().join("cc-switch-web-uploads");
    if let Err(error) = tokio::fs::create_dir_all(&dir).await {
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            format!("创建上传目录失败: {error}"),
        );
    }
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(error) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "bad_request",
                    format!("解析上传内容失败: {error}"),
                )
            }
        };
        if field.name() != Some("file") {
            continue;
        }
        let original = field.file_name().unwrap_or("upload.bin").to_string();
        let safe_name: String = original
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("upload.bin")
            .chars()
            .map(|c| if c.is_control() { '_' } else { c })
            .collect();
        let data = match field.bytes().await {
            Ok(data) => data,
            Err(error) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "bad_request",
                    format!("读取上传内容失败: {error}"),
                )
            }
        };
        let file_name = format!("{}-{}", uuid::Uuid::new_v4(), safe_name);
        let path = dir.join(file_name);
        if let Err(error) = tokio::fs::write(&path, &data).await {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                format!("写入上传文件失败: {error}"),
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(
                &path,
                std::fs::Permissions::from_mode(0o600),
            );
        }
        return Json(json!({
            "path": path.to_string_lossy(),
            "size": data.len(),
        }))
        .into_response();
    }
    json_error(StatusCode::BAD_REQUEST, "bad_request", "缺少上传文件")
}

async fn sse(
    State(state): State<Arc<HttpState>>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let mut receiver = state.ctx.events.subscribe();
    let stream = async_stream::stream! {
        let mut heartbeat = tokio::time::interval(Duration::from_secs(20));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = heartbeat.tick() => {
                    yield Ok(Event::default().comment("heartbeat"));
                }
                message = receiver.recv() => {
                    match message {
                        Ok(data) => yield Ok(Event::default().data(data)),
                        Err(broadcast::error::RecvError::Lagged(skipped)) => {
                            let data = json!({
                                "event": "resync",
                                "payload": { "skipped": skipped },
                            })
                            .to_string();
                            yield Ok(Event::default().data(data));
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn build_csp(index_path: &Path) -> Option<String> {
    let html = std::fs::read_to_string(index_path).ok()?;
    let script_tag = regex::Regex::new(r"(?s)<script([^>]*)>(.*?)</script>").ok()?;
    let mut script_sources = vec!["'self'".to_string()];
    for capture in script_tag.captures_iter(&html) {
        let attributes = capture.get(1).map(|m| m.as_str()).unwrap_or("");
        if attributes.split_whitespace().any(|attr| {
            attr.to_ascii_lowercase().starts_with("src=")
                || attr.to_ascii_lowercase().starts_with("src =")
        }) {
            continue;
        }
        let content = capture.get(2).map(|m| m.as_str()).unwrap_or("");
        if content.trim().is_empty() {
            continue;
        }
        let digest = Sha256::digest(content.as_bytes());
        let hash = base64::engine::general_purpose::STANDARD.encode(digest);
        script_sources.push(format!("'sha256-{hash}'"));
    }
    Some(format!(
        "default-src 'self'; script-src {}; style-src 'self' 'unsafe-inline'; \
         img-src 'self' data: blob:; font-src 'self' data:; \
         connect-src 'self' https://models.dev; \
         frame-ancestors 'none'; base-uri 'self'; form-action 'self'",
        script_sources.join(" ")
    ))
}
