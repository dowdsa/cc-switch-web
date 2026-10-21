mod auth;
mod events;
mod rpc;
mod server;

use std::path::PathBuf;
use std::sync::Arc;

use events::EventBus;
use rpc::WebCtx;

#[derive(Clone, Copy, PartialEq)]
pub enum CookieSecure {
    Auto,
    Always,
    Never,
}

pub struct WebOptions {
    pub host: String,
    pub port: u16,
    pub dist_dir: PathBuf,
    pub allowed_origins: Vec<String>,
    pub cookie_secure: CookieSecure,
}

impl WebOptions {
    fn from_env_and_args() -> Result<Self, String> {
        let mut host = std::env::var("CC_SWITCH_WEB_HOST").unwrap_or_else(|_| "127.0.0.1".into());
        let mut port: u16 = std::env::var("CC_SWITCH_WEB_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8787);
        let mut dist_dir = std::env::var("CC_SWITCH_WEB_DIST")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("dist-web"));
        let mut allowed_origins: Vec<String> = std::env::var("CC_SWITCH_WEB_ALLOWED_ORIGINS")
            .ok()
            .map(|v| {
                v.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let mut cookie_secure = match std::env::var("CC_SWITCH_WEB_COOKIE_SECURE")
            .unwrap_or_else(|_| "auto".into())
            .as_str()
        {
            "true" | "always" => CookieSecure::Always,
            "false" | "never" => CookieSecure::Never,
            _ => CookieSecure::Auto,
        };

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            let (flag, inline_value) = match arg.split_once('=') {
                Some((flag, value)) => (flag.to_string(), Some(value.to_string())),
                None => (arg, None),
            };
            let mut value = || {
                inline_value
                    .clone()
                    .or_else(|| args.next())
                    .ok_or_else(|| format!("{flag} 缺少取值"))
            };
            match flag.as_str() {
                "--host" => host = value()?,
                "--port" => {
                    port = value()?
                        .parse()
                        .map_err(|_| "--port 必须是 0-65535 的整数".to_string())?
                }
                "--dist" => dist_dir = PathBuf::from(value()?),
                "--allowed-origin" => allowed_origins.push(value()?),
                "--cookie-secure" => {
                    cookie_secure = match value()?.as_str() {
                        "true" | "always" => CookieSecure::Always,
                        "false" | "never" => CookieSecure::Never,
                        _ => CookieSecure::Auto,
                    }
                }
                "--web" | "--web-reset-password" => {}
                _ => { /* 未知参数交给桌面模式或忽略 */ }
            }
        }

        if !is_loopback_host(&host) && allowed_origins.is_empty() {
            return Err(format!(
                "拒绝监听非回环地址 {host}：请通过反向代理访问，或显式设置 \
                 CC_SWITCH_WEB_ALLOWED_ORIGINS（如 https://cc.example.com）以确认风险"
            ));
        }

        Ok(Self {
            host,
            port,
            dist_dir,
            allowed_origins,
            cookie_secure,
        })
    }
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "::1" | "localhost")
        || host
            .parse::<std::net::IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
}

pub fn run(reset_password: bool) {
    if let Ok(home) = std::env::var("CC_SWITCH_WEB_HOME") {
        if !home.trim().is_empty() {
            // dirs/settings 都以 HOME 为基准，必须在任何路径解析之前生效。
            std::env::set_var("HOME", home);
        }
    }

    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .try_init();

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("[cc-switch-web] 无法创建异步运行时: {error}");
            std::process::exit(1);
        }
    };

    if reset_password {
        runtime.block_on(async {
            match reset_password_flow().await {
                Ok(username) => {
                    println!("已重置管理员 `{username}` 的密码，所有会话已失效。");
                }
                Err(error) => {
                    eprintln!("重置密码失败: {error}");
                    std::process::exit(1);
                }
            }
        });
        return;
    }

    let options = match WebOptions::from_env_and_args() {
        Ok(options) => options,
        Err(error) => {
            eprintln!("[cc-switch-web] {error}");
            std::process::exit(1);
        }
    };

    runtime.block_on(async move {
        if let Err(error) = run_server(options).await {
            eprintln!("[cc-switch-web] 启动失败: {error}");
            std::process::exit(1);
        }
    });
}

async fn reset_password_flow() -> Result<String, String> {
    use std::io::Write;

    let config_dir = crate::config::get_app_config_dir();
    let auth = auth::AuthStore::init(&config_dir.join("web"))
        .map_err(|e| e.to_string())?;
    print!("新密码（输入后回车）: ");
    let _ = std::io::stdout().flush();
    let mut password = String::new();
    std::io::stdin()
        .read_line(&mut password)
        .map_err(|e| format!("读取密码失败: {e}"))?;
    let password = password.trim_end_matches(['\r', '\n']).to_string();
    auth.reset_password(&password).await.map_err(|e| e.to_string())
}

async fn run_server(options: WebOptions) -> Result<(), String> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    crate::panic_hook::setup_panic_hook();

    let config_dir = crate::config::get_app_config_dir();
    std::fs::create_dir_all(&config_dir)
        .map_err(|e| format!("创建配置目录 {} 失败: {e}", config_dir.display()))?;
    crate::panic_hook::init_app_config_dir(config_dir.clone());

    let db_path = config_dir.join("cc-switch.db");
    if let Ok(Some(version)) = crate::database::Database::stored_user_version_exceeds_supported(&db_path) {
        let message = format!(
            "数据库版本过新（{version}），当前应用仅支持 {}，请升级应用后再启动。",
            crate::database::SCHEMA_VERSION
        );
        crate::init_status::set_init_error(crate::init_status::InitErrorPayload {
            path: db_path.display().to_string(),
            error: message.clone(),
            kind: Some("db_version_too_new".into()),
            db_version: Some(version),
            supported_version: Some(crate::database::SCHEMA_VERSION),
        });
        return Err(message);
    }

    // 旧版 config.json → SQLite（无交互版本：失败直接如实报错，不落半套数据）。
    let json_path = config_dir.join("config.json");
    let migration_config = if !db_path.exists() && json_path.exists() {
        log::info!("检测到旧版 config.json，开始迁移到 SQLite");
        Some(
            crate::app_config::MultiAppConfig::load()
                .map_err(|e| format!("读取旧版配置文件失败: {e}"))?,
        )
    } else {
        None
    };

    let db = Arc::new(crate::database::Database::init().map_err(|e| e.to_string())?);

    if let Some(config) = migration_config {
        db.migrate_from_json(&config)
            .map_err(|e| format!("迁移旧版配置失败: {e}"))?;
        crate::init_status::set_migration_success();
        let archive = json_path.with_extension("json.migrated");
        if let Err(e) = std::fs::rename(&json_path, &archive) {
            log::warn!("归档旧版配置文件失败: {e}");
        } else {
            log::info!("旧版配置已归档为 {}", archive.display());
        }
    }

    let state = crate::store::AppState::new(db.clone());

    crate::mode::operation::recover_on_startup(&state.db);

    if let Err(e) = db.init_default_skill_repos() {
        log::warn!("初始化默认 Skills 仓库失败: {e}");
    }

    match db.get_setting("skills_ssot_migration_pending") {
        Ok(Some(flag)) if flag == "true" || flag == "1" => {
            let has_existing = db
                .get_all_installed_skills()
                .map(|skills| !skills.is_empty())
                .unwrap_or(false);
            if has_existing {
                let _ = db.set_setting("skills_ssot_migration_pending", "false");
            } else {
                match crate::services::skill::migrate_skills_to_ssot(&db) {
                    Ok(count) => {
                        if count > 0 {
                            log::info!("已自动导入 {count} 个 Skills 到统一目录");
                        }
                        let _ = db.set_setting("skills_ssot_migration_pending", "false");
                    }
                    Err(e) => log::warn!("自动导入旧版 Skills 失败: {e}"),
                }
            }
        }
        _ => {}
    }

    for app in crate::app_config::AppType::all().filter(|t| !t.is_additive_mode()) {
        if !crate::services::provider::should_import_default_config_on_startup(&state, &app)
            .unwrap_or(false)
        {
            continue;
        }
        match crate::services::provider::ProviderService::import_default_config(&state, app) {
            Ok(true) => log::info!("已导入 live 配置作为默认供应商"),
            Ok(false) => {}
            Err(e) => log::debug!("没有可导入的 live 配置: {e}"),
        }
    }

    if let Err(e) = db.init_default_official_providers() {
        log::warn!("写入官方预设供应商失败: {e}");
    }

    let proxy_url = db.get_global_proxy_url().ok().flatten();
    if let Err(e) = crate::proxy::http_client::init(proxy_url.as_deref()) {
        log::error!("初始化全局代理客户端失败: {e}");
        if proxy_url.is_some() {
            let _ = db.set_global_proxy_url(None);
        }
        if let Err(fallback) = crate::proxy::http_client::init(None) {
            log::error!("回退直连模式失败: {fallback}");
        }
    }

    crate::mode::controller::startup(&state).await;

    if let Err(e) = db.periodic_backup_if_needed() {
        log::warn!("启动时定期备份失败: {e}");
    }

    let auth = auth::AuthStore::init(&config_dir.join("web"))
        .map_err(|e| e.to_string())?;
    if let Some(token) = auth.ensure_setup_token() {
        let token_path = config_dir.join("web").join("setup-token");
        println!("============================================================");
        println!("  CC Switch Web 首次初始化");
        println!("  初始化令牌 (setup token): {token}");
        println!("  已写入: {}", token_path.display());
        println!("  打开 Web 界面后用它创建管理员账号，完成后令牌作废。");
        println!("============================================================");
    }

    let copilot = Arc::new(tokio::sync::RwLock::new(
        crate::proxy::providers::copilot_auth::CopilotAuthManager::new(config_dir.clone()),
    ));
    let xai = Arc::new(tokio::sync::RwLock::new(
        crate::proxy::providers::xai_oauth_auth::XaiOAuthManager::new(config_dir.clone()),
    ));

    let ctx = Arc::new(WebCtx {
        state,
        auth,
        events: EventBus::new(256),
        config_dir,
        skills: Arc::new(crate::services::skill::SkillService::new()),
        copilot,
        xai,
    });

    server::serve(ctx, options).await
}
