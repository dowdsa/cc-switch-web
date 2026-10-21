use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use axum::http::StatusCode;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::app_config::{AppType, McpServer};
use crate::commands::{CurrentProfileIds, ProfileDto, ProfilesResponse};
use crate::error::AppError;
use crate::services::model_pricing::ModelPricingInfo;
use crate::services::usage_stats::LogFilters;
use crate::prompt::Prompt;
use crate::provider::{Provider, ProviderMeta, UniversalProvider};
use crate::services::pi_prompt_files::{PiPromptFileKind, PiPromptFileService, PiPromptTemplateService};
use crate::services::profile::{ProfileScope, ProfileService};
use crate::services::prompt::PromptService;
use crate::services::provider::icon;
use crate::services::provider::EditorSave;
use crate::services::skill::{
    DiscoverableSkill, ImportSkillSelection, SkillRepo, SkillService, SkillStorageLocation,
};
use crate::services::McpService;
use crate::settings::{get_settings, get_settings_for_frontend, update_settings, AppSettings};
use crate::store::AppState;
use crate::web::auth::AuthStore;
use crate::web::events::EventBus;

pub struct WebCtx {
    pub state: AppState,
    pub auth: AuthStore,
    pub events: EventBus,
    pub config_dir: PathBuf,
    pub skills: Arc<SkillService>,
    pub copilot: Arc<tokio::sync::RwLock<crate::proxy::providers::copilot_auth::CopilotAuthManager>>,
    pub xai: Arc<tokio::sync::RwLock<crate::proxy::providers::xai_oauth_auth::XaiOAuthManager>>,
}

#[derive(Debug)]
pub enum RpcError {
    BadRequest(String),
    Unsupported(String),
    Internal(String),
}

impl RpcError {
    pub fn parts(&self) -> (StatusCode, &'static str, String) {
        match self {
            RpcError::BadRequest(message) => {
                (StatusCode::BAD_REQUEST, "bad_request", message.clone())
            }
            RpcError::Unsupported(command) => (
                StatusCode::NOT_IMPLEMENTED,
                "unsupported",
                format!("命令 `{command}` 尚未在 Web 模式中实现"),
            ),
            RpcError::Internal(message) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "internal", message.clone())
            }
        }
    }
}

impl From<crate::error::AppError> for RpcError {
    fn from(error: crate::error::AppError) -> Self {
        RpcError::Internal(error.to_string())
    }
}

impl From<String> for RpcError {
    fn from(error: String) -> Self {
        RpcError::Internal(error)
    }
}

impl From<anyhow::Error> for RpcError {
    fn from(error: anyhow::Error) -> Self {
        RpcError::Internal(error.to_string())
    }
}

impl From<rusqlite::Error> for RpcError {
    fn from(error: rusqlite::Error) -> Self {
        RpcError::Internal(error.to_string())
    }
}

fn arg<T: DeserializeOwned>(args: &Value, name: &str) -> Result<T, RpcError> {
    let value = args
        .get(name)
        .cloned()
        .ok_or_else(|| RpcError::BadRequest(format!("缺少参数: {name}")))?;
    serde_json::from_value(value)
        .map_err(|e| RpcError::BadRequest(format!("参数 {name} 格式错误: {e}")))
}

fn optional_arg<T: DeserializeOwned>(args: &Value, name: &str) -> Result<Option<T>, RpcError> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|e| RpcError::BadRequest(format!("参数 {name} 格式错误: {e}"))),
    }
}

fn app_type(app: &str) -> Result<AppType, RpcError> {
    AppType::from_str(app).map_err(|e| RpcError::BadRequest(e.to_string()))
}

fn require_proxy_app(app: &str) -> Result<AppType, RpcError> {
    let app_type = app_type(app)?;
    if !app_type.supports_local_proxy() {
        return Err(RpcError::BadRequest(format!(
            "{} 不支持本地路由",
            app_type.as_str()
        )));
    }
    Ok(app_type)
}

fn require_mode_app(app: &str) -> Result<AppType, RpcError> {
    let app_type = app_type(app)?;
    if !app_type.has_route_mode() {
        return Err(RpcError::BadRequest(format!(
            "{} 不支持本地路由",
            app_type.as_str()
        )));
    }
    Ok(app_type)
}

#[allow(deprecated)]
pub async fn dispatch(ctx: &WebCtx, command: &str, args: Value) -> Result<Value, RpcError> {
    match command {
        "get_init_error" => Ok(serde_json::to_value(crate::init_status::get_init_error())
            .map_err(|e| RpcError::Internal(e.to_string()))?),
        "get_settings" => Ok(serde_json::to_value(get_settings_for_frontend())
            .map_err(|e| RpcError::Internal(e.to_string()))?),
        "save_settings" => save_settings(ctx, args).await,
        "get_providers" => {
            let app = arg::<String>(&args, "app")?;
            let providers = crate::services::ProviderService::list(&ctx.state, app_type(&app)?)?;
            Ok(serde_json::to_value(providers).map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_current_provider" => {
            let app = arg::<String>(&args, "app")?;
            let current =
                crate::services::ProviderService::current(&ctx.state, app_type(&app)?)?;
            Ok(json!(current))
        }
        "get_universal_providers" => {
            let providers = crate::services::ProviderService::list_universal(&ctx.state)?;
            Ok(serde_json::to_value(providers).map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "add_provider" => {
            let app = arg::<String>(&args, "app")?;
            let provider = arg::<Provider>(&args, "provider")?;
            let add_to_live = args
                .get("addToLive")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let editor_save = optional_arg::<EditorSave>(&args, "editorSave")?;
            let added = crate::services::ProviderService::add_from_editor(
                &ctx.state,
                app_type(&app)?,
                provider,
                add_to_live,
                editor_save,
            )?;
            Ok(json!(added))
        }
        "update_provider" => {
            let app = arg::<String>(&args, "app")?;
            let provider = arg::<Provider>(&args, "provider")?;
            let original_id = optional_arg::<String>(&args, "originalId")?;
            let editor_save = optional_arg::<EditorSave>(&args, "editorSave")?;
            let updated = crate::services::ProviderService::update_from_editor(
                &ctx.state,
                app_type(&app)?,
                original_id.as_deref(),
                provider,
                editor_save,
            )?;
            Ok(json!(updated))
        }
        "delete_provider" => {
            let app = arg::<String>(&args, "app")?;
            let id = arg::<String>(&args, "id")?;
            crate::services::ProviderService::delete(&ctx.state, app_type(&app)?, &id)?;
            Ok(json!(true))
        }
        "remove_provider_from_live_config" => {
            let app = arg::<String>(&args, "app")?;
            let id = arg::<String>(&args, "id")?;
            crate::services::ProviderService::remove_from_live_config(
                &ctx.state,
                app_type(&app)?,
                &id,
            )?;
            Ok(json!(true))
        }
        "switch_provider" => {
            let app = arg::<String>(&args, "app")?;
            let id = arg::<String>(&args, "id")?;
            let result =
                crate::services::ProviderService::switch(&ctx.state, app_type(&app)?, &id)?;
            ctx.events.publish(
                "provider-switched",
                json!({ "appType": app, "providerId": id }),
            );
            Ok(serde_json::to_value(result).map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "update_providers_sort_order" => {
            let app = arg::<String>(&args, "app")?;
            let updates: Vec<crate::services::ProviderSortUpdate> = arg(&args, "updates")?;
            let changed = crate::services::ProviderService::update_sort_order(
                &ctx.state,
                app_type(&app)?,
                updates,
            )?;
            Ok(json!(changed))
        }
        "get_provider_editor_view" => {
            let app = arg::<String>(&args, "app")?;
            let app_type = app_type(&app)?;
            let settings_config = arg::<Value>(&args, "settingsConfig")?;
            let category = optional_arg::<String>(&args, "category")?;
            let provider_id = optional_arg::<String>(&args, "providerId")?;
            let meta = optional_arg::<ProviderMeta>(&args, "meta")?;
            let category = crate::services::ProviderService::editor_category(
                &ctx.state,
                &app_type,
                provider_id.as_deref(),
                category,
            )?;
            let view = crate::services::ProviderService::editor_view_with_meta(
                &ctx.state,
                app_type,
                &settings_config,
                category.as_deref(),
                provider_id.as_deref(),
                meta.as_ref(),
            )?;
            Ok(serde_json::to_value(view).map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "import_default_config" => {
            let app = arg::<String>(&args, "app")?;
            let imported = crate::services::provider::import_default_config(
                &ctx.state,
                app_type(&app)?,
            )?;
            Ok(json!(imported))
        }
        "ensure_codex_official_provider" => {
            let created = ctx.state.db.ensure_official_seed_by_id(
                crate::database::CODEX_OFFICIAL_PROVIDER_ID,
                AppType::Codex,
            )?;
            Ok(json!(created))
        }
        "ensure_grokbuild_official_provider" => {
            let created = ctx.state.db.ensure_official_seed_by_id(
                crate::database::GROKBUILD_OFFICIAL_PROVIDER_ID,
                AppType::GrokBuild,
            )?;
            Ok(json!(created))
        }
        "ensure_claude_desktop_official_provider" => {
            let created = ctx.state.db.ensure_official_seed_by_id(
                crate::database::CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID,
                AppType::ClaudeDesktop,
            )?;
            Ok(json!(created))
        }
        "get_universal_provider" => {
            let id = arg::<String>(&args, "id")?;
            let provider = crate::services::ProviderService::get_universal(&ctx.state, &id)?;
            Ok(serde_json::to_value(provider).map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "upsert_universal_provider" => {
            let provider = arg::<UniversalProvider>(&args, "provider")?;
            let id = provider.id.clone();
            let changed =
                crate::services::ProviderService::upsert_universal(&ctx.state, provider)?;
            ctx.events.publish(
                "universal-provider-synced",
                json!({ "action": "upsert", "id": id }),
            );
            Ok(json!(changed))
        }
        "delete_universal_provider" => {
            let id = arg::<String>(&args, "id")?;
            let changed =
                crate::services::ProviderService::delete_universal(&ctx.state, &id)?;
            ctx.events.publish(
                "universal-provider-synced",
                json!({ "action": "delete", "id": id }),
            );
            Ok(json!(changed))
        }
        "sync_universal_provider" => {
            let id = arg::<String>(&args, "id")?;
            let changed =
                crate::services::ProviderService::sync_universal_to_apps(&ctx.state, &id)?;
            ctx.events.publish(
                "universal-provider-synced",
                json!({ "action": "sync", "id": id }),
            );
            Ok(json!(changed))
        }
        "get_custom_icons" => {
            let icons = icon::list_library_icons(&ctx.state.db)?;
            Ok(serde_json::to_value(icons).map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "add_custom_icon" => {
            let name = arg::<String>(&args, "name")?;
            let data = arg::<String>(&args, "data")?;
            let result = icon::add_library_icon(&ctx.state.db, &name, &data)?;
            Ok(serde_json::to_value(result).map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "rename_custom_icon" => {
            let id = arg::<String>(&args, "id")?;
            let name = arg::<String>(&args, "name")?;
            let entry = icon::rename_library_icon(&ctx.state.db, &id, &name)?;
            Ok(serde_json::to_value(entry).map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "delete_custom_icon" => {
            let id = arg::<String>(&args, "id")?;
            icon::delete_library_icon(&ctx.state.db, &id)?;
            Ok(json!(true))
        }
        "list_profiles" => {
            let profiles = ProfileService::list(&ctx.state)?;
            let current_ids = CurrentProfileIds {
                claude: ctx
                    .state
                    .db
                    .get_current_profile_id(ProfileScope::Claude.as_str())?,
                claude_desktop: ctx
                    .state
                    .db
                    .get_current_profile_id(ProfileScope::ClaudeDesktop.as_str())?,
                codex: ctx
                    .state
                    .db
                    .get_current_profile_id(ProfileScope::Codex.as_str())?,
            };
            let response = ProfilesResponse {
                profiles: profiles.into_iter().map(ProfileDto::from).collect(),
                current_ids,
            };
            Ok(serde_json::to_value(response).map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "create_profile" => {
            let name = arg::<String>(&args, "name")?;
            let scope = ProfileScope::parse(&arg::<String>(&args, "scope")?)
                .map_err(|e| RpcError::BadRequest(e.to_string()))?;
            let profile = ProfileService::create(&ctx.state, &name, scope)?;
            Ok(serde_json::to_value(ProfileDto::from(profile))
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "update_profile" => {
            let id = arg::<String>(&args, "id")?;
            let name = optional_arg::<String>(&args, "name")?;
            let resnapshot = args
                .get("resnapshot")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let scope = optional_arg::<String>(&args, "scope")?
                .map(|value| ProfileScope::parse(&value))
                .transpose()
                .map_err(|e| RpcError::BadRequest(e.to_string()))?;
            let profile = ProfileService::update(&ctx.state, &id, name, resnapshot, scope)?;
            Ok(serde_json::to_value(ProfileDto::from(profile))
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "delete_profile" => {
            let id = arg::<String>(&args, "id")?;
            ProfileService::delete(&ctx.state, &id)?;
            Ok(json!(null))
        }
        "clear_current_profile" => {
            let scope = ProfileScope::parse(&arg::<String>(&args, "scope")?)
                .map_err(|e| RpcError::BadRequest(e.to_string()))?;
            ctx.state.db.set_current_profile_id(scope.as_str(), None)?;
            Ok(json!(null))
        }
        "apply_profile" => {
            let id = arg::<String>(&args, "id")?;
            let scope = ProfileScope::parse(&arg::<String>(&args, "scope")?)
                .map_err(|e| RpcError::BadRequest(e.to_string()))?;
            let warnings = ProfileService::apply(&ctx.state, &id, scope)?;
            publish_profile_events(ctx, &id, scope);
            Ok(serde_json::to_value(warnings)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_global_proxy_url" => {
            Ok(json!(ctx.state.db.get_global_proxy_url()?))
        }
        "set_global_proxy_url" => {
            let url = arg::<String>(&args, "url")?;
            let url_opt = if url.trim().is_empty() {
                None
            } else {
                Some(url.as_str())
            };
            crate::proxy::http_client::validate_proxy(url_opt)
                .map_err(RpcError::BadRequest)?;
            ctx.state.db.set_global_proxy_url(url_opt)?;
            crate::proxy::http_client::apply_proxy(url_opt)
                .map_err(RpcError::Internal)?;
            Ok(json!(null))
        }
        "get_upstream_proxy_status" => {
            let url = crate::proxy::http_client::get_current_proxy_url();
            Ok(json!({ "enabled": url.is_some(), "proxyUrl": url }))
        }
        "check_env_conflicts" => {
            let app = arg::<String>(&args, "app")?;
            let conflicts = crate::services::env_checker::check_env_conflicts(&app)?;
            Ok(serde_json::to_value(conflicts).map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "delete_env_vars" => {
            let conflicts: Vec<crate::services::env_checker::EnvConflict> =
                arg(&args, "conflicts")?;
            let info = crate::services::env_manager::delete_env_vars(conflicts)?;
            Ok(serde_json::to_value(info).map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "restore_env_backup" => {
            let backup_path = arg::<String>(&args, "backupPath")?;
            crate::services::env_manager::restore_from_backup(backup_path)?;
            Ok(json!(null))
        }
        "get_claude_code_config_path" => Ok(json!(
            crate::config::get_claude_settings_path()
                .to_string_lossy()
                .to_string()
        )),
        "get_app_config_path" => Ok(json!(
            crate::config::get_app_config_path().to_string_lossy().to_string()
        )),
        "get_claude_config_status" => {
            let status = crate::config::get_claude_config_status();
            Ok(serde_json::to_value(status).map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_config_dir" => {
            let app = arg::<String>(&args, "app")?;
            let dir = match app_type(&app)? {
                AppType::Claude => crate::config::get_claude_config_dir(),
                AppType::ClaudeDesktop => crate::claude_desktop_config::get_config_library_path()
                    .map_err(|e| RpcError::Internal(e.to_string()))?,
                AppType::Codex => crate::codex_config::get_codex_config_dir(),
                AppType::Gemini => crate::gemini_config::get_gemini_dir(),
                AppType::GrokBuild => crate::grok_config::get_grok_config_dir(),
                AppType::OpenCode => crate::opencode_config::get_opencode_dir(),
                AppType::OpenClaw => crate::openclaw_config::get_openclaw_dir(),
                AppType::Hermes => crate::hermes_config::get_hermes_dir(),
                AppType::Pi => crate::pi_config::get_pi_agent_dir()
                    .map_err(|e| RpcError::Internal(e.to_string()))?,
                AppType::Mcode => crate::mcode_config::config_path()
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_default(),
            };
            Ok(json!(dir.to_string_lossy().to_string()))
        }
        "get_opencode_live_provider_ids" => {
            let providers = crate::opencode_config::get_providers()
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(providers.keys().cloned().collect::<Vec<_>>()))
        }
        "get_opencode_default_model" => Ok(json!(
            crate::opencode_config::get_default_model()
                .map_err(|e| RpcError::Internal(e.to_string()))?
        )),
        "set_opencode_default_model" => {
            let model = arg::<String>(&args, "model")?;
            crate::opencode_config::set_default_model(&model)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(null))
        }
        "get_claude_mcp_status" => Ok(serde_json::to_value(
            crate::claude_mcp::get_mcp_status()?,
        )
        .map_err(|e| RpcError::Internal(e.to_string()))?),
        "read_claude_mcp_config" => Ok(json!(crate::claude_mcp::read_mcp_json()?)),
        "upsert_claude_mcp_server" => {
            let id = arg::<String>(&args, "id")?;
            let spec = arg::<Value>(&args, "spec")?;
            Ok(json!(crate::claude_mcp::upsert_mcp_server(&id, spec)?))
        }
        "delete_claude_mcp_server" => {
            let id = arg::<String>(&args, "id")?;
            Ok(json!(crate::claude_mcp::delete_mcp_server(&id)?))
        }
        "validate_mcp_command" => {
            let cmd = arg::<String>(&args, "cmd")?;
            Ok(json!(crate::claude_mcp::validate_command_in_path(&cmd)?))
        }
        "get_mcp_config" => {
            let app = arg::<String>(&args, "app")?;
            let servers = McpService::get_servers(&ctx.state, app_type(&app)?)?;
            Ok(json!({
                "configPath": crate::config::get_app_config_path().to_string_lossy().to_string(),
                "servers": servers,
            }))
        }
        "upsert_mcp_server_in_config" => {
            let app = arg::<String>(&args, "app")?;
            let id = arg::<String>(&args, "id")?;
            let spec = arg::<Value>(&args, "spec")?;
            let sync_other_side = args
                .get("syncOtherSide")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let app_ty = app_type(&app)?;
            let existing = ctx.state.db.get_all_mcp_servers()?.get(&id).cloned();
            let mut server = if let Some(mut existing) = existing {
                existing.server = spec.clone();
                existing.apps.set_enabled_for(&app_ty, true);
                existing
            } else {
                let mut apps = crate::app_config::McpApps::default();
                apps.set_enabled_for(&app_ty, true);
                let name = spec
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(&id)
                    .to_string();
                McpServer {
                    id: id.clone(),
                    name,
                    server: spec,
                    apps,
                    description: None,
                    homepage: None,
                    docs: None,
                    tags: Vec::new(),
                }
            };
            if sync_other_side {
                server.apps.claude = true;
                server.apps.codex = true;
                server.apps.gemini = true;
                server.apps.opencode = true;
            }
            McpService::upsert_server(&ctx.state, server)?;
            Ok(json!(true))
        }
        "delete_mcp_server_in_config" => {
            let id = arg::<String>(&args, "id")?;
            Ok(json!(McpService::delete_server(&ctx.state, &id)?))
        }
        "set_mcp_enabled" => {
            let app = arg::<String>(&args, "app")?;
            let id = arg::<String>(&args, "id")?;
            let enabled = arg::<bool>(&args, "enabled")?;
            Ok(json!(McpService::set_enabled(
                &ctx.state,
                app_type(&app)?,
                &id,
                enabled
            )?))
        }
        "get_mcp_servers" => {
            let servers = McpService::get_all_servers(&ctx.state)?;
            Ok(serde_json::to_value(servers).map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "upsert_mcp_server" => {
            let server = arg::<McpServer>(&args, "server")?;
            McpService::upsert_server(&ctx.state, server)?;
            Ok(json!(null))
        }
        "delete_mcp_server" => {
            let id = arg::<String>(&args, "id")?;
            Ok(json!(McpService::delete_server(&ctx.state, &id)?))
        }
        "toggle_mcp_app" => {
            let server_id = arg::<String>(&args, "serverId")?;
            let app = arg::<String>(&args, "app")?;
            let enabled = arg::<bool>(&args, "enabled")?;
            McpService::toggle_app(&ctx.state, &server_id, app_type(&app)?, enabled)?;
            Ok(json!(null))
        }
        "import_mcp_from_apps" => {
            Ok(json!(McpService::import_from_all_apps(&ctx.state)?))
        }
        "resync_mcp_to_apps" => {
            let apps: Option<Vec<String>> = optional_arg(&args, "apps")?;
            let targets = McpService::resync_targets(apps.as_deref())?;
            let mut outcomes = Vec::with_capacity(targets.len());
            for app in targets {
                let _guard = ctx.state.proxy_service.lock_switch_for_app(app.as_str()).await;
                outcomes.push(McpService::resync_app(&ctx.state, &app));
            }
            Ok(serde_json::to_value(outcomes)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_prompts" => {
            let app = arg::<String>(&args, "app")?;
            let prompts = PromptService::get_prompts(&ctx.state, app_type(&app)?)?;
            Ok(serde_json::to_value(prompts).map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "upsert_prompt" => {
            let app = arg::<String>(&args, "app")?;
            let id = arg::<String>(&args, "id")?;
            let prompt = arg::<Prompt>(&args, "prompt")?;
            PromptService::upsert_prompt(&ctx.state, app_type(&app)?, &id, prompt)?;
            Ok(json!(null))
        }
        "delete_prompt" => {
            let app = arg::<String>(&args, "app")?;
            let id = arg::<String>(&args, "id")?;
            PromptService::delete_prompt(&ctx.state, app_type(&app)?, &id)?;
            Ok(json!(null))
        }
        "enable_prompt" => {
            let app = arg::<String>(&args, "app")?;
            let id = arg::<String>(&args, "id")?;
            PromptService::enable_prompt(&ctx.state, app_type(&app)?, &id)?;
            Ok(json!(null))
        }
        "get_current_prompt_file_content" => {
            let app = arg::<String>(&args, "app")?;
            Ok(json!(PromptService::get_current_file_content(app_type(&app)?)?))
        }
        "get_prompt_file_location" => {
            let app = arg::<String>(&args, "app")?;
            let path = crate::prompt_files::prompt_file_path(&app_type(&app)?)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!({
                "path": path.to_string_lossy().to_string(),
                "displayPath": crate::prompt_files::display_path(&path),
            }))
        }
        "get_pi_prompt_file" => {
            let kind = arg::<PiPromptFileKind>(&args, "kind")?;
            let snapshot = PiPromptFileService::read(kind)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(snapshot).map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "replace_pi_prompt_file" => {
            let kind = arg::<PiPromptFileKind>(&args, "kind")?;
            let expected = arg::<String>(&args, "expectedRevision")?;
            let content = arg::<String>(&args, "content")?;
            let snapshot = PiPromptFileService::replace(kind, &expected, &content)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(snapshot).map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "delete_pi_prompt_file" => {
            let kind = arg::<PiPromptFileKind>(&args, "kind")?;
            let expected = arg::<String>(&args, "expectedRevision")?;
            Ok(json!(PiPromptFileService::delete(kind, &expected)
                .map_err(|e| RpcError::Internal(e.to_string()))?))
        }
        "list_pi_prompt_templates" => {
            let templates = PiPromptTemplateService::list()
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(templates)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "upsert_pi_prompt_template" => {
            let slug = arg::<String>(&args, "slug")?;
            let original_slug = optional_arg::<String>(&args, "originalSlug")?;
            let expected = arg::<String>(&args, "expectedRevision")?;
            let content = arg::<String>(&args, "content")?;
            let template = PiPromptTemplateService::upsert(
                &slug,
                original_slug.as_deref(),
                &expected,
                &content,
            )
            .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(template)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "delete_pi_prompt_template" => {
            let slug = arg::<String>(&args, "slug")?;
            let expected = arg::<String>(&args, "expectedRevision")?;
            Ok(json!(PiPromptTemplateService::delete(&slug, &expected)
                .map_err(|e| RpcError::Internal(e.to_string()))?))
        }
        "get_installed_skills" => {
            let skills = SkillService::get_all_installed(&ctx.state.db)?;
            Ok(serde_json::to_value(skills).map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_skill_backups" => {
            let backups = SkillService::list_backups()?;
            Ok(serde_json::to_value(backups).map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "delete_skill_backup" => {
            let backup_id = arg::<String>(&args, "backupId")?;
            SkillService::delete_backup(&backup_id)?;
            Ok(json!(true))
        }
        "install_skill_unified" => {
            let skill = arg::<DiscoverableSkill>(&args, "skill")?;
            let current_app = arg::<String>(&args, "currentApp")?;
            let installed = ctx
                .skills
                .install(&ctx.state.db, &skill, &app_type(&current_app)?)
                .await?;
            Ok(serde_json::to_value(installed)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "uninstall_skill_unified" => {
            let id = arg::<String>(&args, "id")?;
            let result = SkillService::uninstall(&ctx.state.db, &id)?;
            Ok(serde_json::to_value(result).map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "restore_skill_backup" => {
            let backup_id = arg::<String>(&args, "backupId")?;
            let current_app = arg::<String>(&args, "currentApp")?;
            let installed = SkillService::restore_from_backup(
                &ctx.state.db,
                &backup_id,
                &app_type(&current_app)?,
            )?;
            Ok(serde_json::to_value(installed)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "toggle_skill_app" => {
            let id = arg::<String>(&args, "id")?;
            let app = arg::<String>(&args, "app")?;
            let enabled = arg::<bool>(&args, "enabled")?;
            SkillService::toggle_app(&ctx.state.db, &id, &app_type(&app)?, enabled)?;
            Ok(json!(true))
        }
        "scan_unmanaged_skills" => {
            let skills = SkillService::scan_unmanaged(&ctx.state.db)?;
            Ok(serde_json::to_value(skills).map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "import_skills_from_apps" => {
            let imports = arg::<Vec<ImportSkillSelection>>(&args, "imports")?;
            let skills = SkillService::import_from_apps(&ctx.state.db, imports)?;
            Ok(serde_json::to_value(skills).map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "discover_available_skills" => {
            let repos = ctx.state.db.get_skill_repos()?;
            let result = ctx.skills.discover_available_report(repos).await?;
            Ok(serde_json::to_value(result)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "check_skill_updates" => {
            let result = ctx.skills.check_updates_report(&ctx.state.db).await?;
            Ok(serde_json::to_value(result)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "resync_skills_to_apps" => {
            let db = ctx.state.db.clone();
            let outcomes: Vec<crate::services::skill::SkillAppSyncOutcome> =
                tokio::task::spawn_blocking(move || SkillService::resync_all_apps(&db))
                    .await
                    .map_err(|e| RpcError::Internal(format!("重新同步 Skill 失败: {e}")))?;
            Ok(serde_json::to_value(outcomes)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "update_skill" => {
            let id = arg::<String>(&args, "id")?;
            let installed = ctx.skills.update_skill(&ctx.state.db, &id).await?;
            Ok(serde_json::to_value(installed)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "migrate_skill_storage" => {
            let target = arg::<SkillStorageLocation>(&args, "target")?;
            let result = SkillService::migrate_storage(&ctx.state.db, target)?;
            Ok(serde_json::to_value(result).map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_cc_switch_skills_dir" => Ok(json!(
            ctx.config_dir.join("skills").to_string_lossy().to_string()
        )),
        "search_skills_sh" => {
            let query = arg::<String>(&args, "query")?;
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(20) as usize;
            let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
            let result = SkillService::search_skills_sh(&query, limit, offset).await?;
            Ok(serde_json::to_value(result)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_skills" | "get_skills_for_app" => {
            let repos = ctx.state.db.get_skill_repos()?;
            let skills = ctx.skills.list_skills(repos, &ctx.state.db).await?;
            Ok(serde_json::to_value(skills).map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "install_skill" | "install_skill_for_app" => {
            let directory = arg::<String>(&args, "directory")?;
            let app = optional_arg::<String>(&args, "app")?
                .unwrap_or_else(|| "claude".to_string());
            let app_ty = app_type(&app)?;
            let repos = ctx.state.db.get_skill_repos()?;
            let skills = ctx.skills.discover_available(repos).await?;
            let skill = skills
                .into_iter()
                .find(|skill| {
                    let install_name = std::path::Path::new(&skill.directory)
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_else(|| skill.directory.clone());
                    install_name.eq_ignore_ascii_case(&directory)
                        || skill.directory.eq_ignore_ascii_case(&directory)
                })
                .ok_or_else(|| {
                    RpcError::BadRequest(format!("未找到可安装的 Skill: {directory}"))
                })?;
            ctx.skills.install(&ctx.state.db, &skill, &app_ty).await?;
            Ok(json!(true))
        }
        "uninstall_skill" | "uninstall_skill_for_app" => {
            let directory = arg::<String>(&args, "directory")?;
            let skills = SkillService::get_all_installed(&ctx.state.db)?;
            let skill = skills
                .into_iter()
                .find(|skill| skill.directory.eq_ignore_ascii_case(&directory))
                .ok_or_else(|| {
                    RpcError::BadRequest(format!("未找到已安装的 Skill: {directory}"))
                })?;
            let result = SkillService::uninstall(&ctx.state.db, &skill.id)?;
            Ok(serde_json::to_value(result).map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_skill_repos" => {
            let repos = ctx.state.db.get_skill_repos()?;
            Ok(serde_json::to_value(repos).map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "add_skill_repo" => {
            let repo = arg::<SkillRepo>(&args, "repo")?;
            SkillService::validate_repo_ref(&repo.owner, &repo.name, &repo.branch)?;
            ctx.state.db.save_skill_repo(&repo)?;
            Ok(json!(true))
        }
        "remove_skill_repo" => {
            let owner = arg::<String>(&args, "owner")?;
            let name = arg::<String>(&args, "name")?;
            ctx.state.db.delete_skill_repo(&owner, &name)?;
            Ok(json!(true))
        }
        "install_skills_from_zip" => {
            let file_path = arg::<String>(&args, "filePath")?;
            let current_app = arg::<String>(&args, "currentApp")?;
            let result = SkillService::install_from_zip(
                &ctx.state.db,
                std::path::Path::new(&file_path),
                &app_type(&current_app)?,
            )?;
            Ok(serde_json::to_value(result)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_usage_summary" => {
            let summary = ctx.state.db.get_usage_summary(
                optional_arg::<i64>(&args, "startDate")?,
                optional_arg::<i64>(&args, "endDate")?,
                optional_arg::<String>(&args, "appType")?.as_deref(),
                optional_arg::<String>(&args, "providerName")?.as_deref(),
                optional_arg::<String>(&args, "model")?.as_deref(),
            )?;
            Ok(serde_json::to_value(summary)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_session_usage_summary" => {
            let app_type = arg::<String>(&args, "appType")?;
            let session_id = arg::<String>(&args, "sessionId")?;
            let summary = ctx.state.db.get_session_usage_summary(&app_type, &session_id)?;
            Ok(serde_json::to_value(summary)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_usage_summary_by_app" => {
            let rows = ctx.state.db.get_usage_summary_by_app(
                optional_arg::<i64>(&args, "startDate")?,
                optional_arg::<i64>(&args, "endDate")?,
                optional_arg::<String>(&args, "providerName")?.as_deref(),
                optional_arg::<String>(&args, "model")?.as_deref(),
            )?;
            Ok(serde_json::to_value(rows)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_usage_trends" => {
            let rows = ctx.state.db.get_daily_trends(
                optional_arg::<i64>(&args, "startDate")?,
                optional_arg::<i64>(&args, "endDate")?,
                optional_arg::<String>(&args, "appType")?.as_deref(),
                optional_arg::<String>(&args, "providerName")?.as_deref(),
                optional_arg::<String>(&args, "model")?.as_deref(),
            )?;
            Ok(serde_json::to_value(rows)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_usage_first_date" => {
            let date = ctx.state.db.get_first_usage_date(
                optional_arg::<String>(&args, "appType")?.as_deref(),
                optional_arg::<String>(&args, "providerName")?.as_deref(),
                optional_arg::<String>(&args, "model")?.as_deref(),
            )?;
            Ok(json!(date))
        }
        "get_provider_stats" => {
            let rows = ctx.state.db.get_provider_stats(
                optional_arg::<i64>(&args, "startDate")?,
                optional_arg::<i64>(&args, "endDate")?,
                optional_arg::<String>(&args, "appType")?.as_deref(),
                optional_arg::<String>(&args, "providerName")?.as_deref(),
                optional_arg::<String>(&args, "model")?.as_deref(),
            )?;
            Ok(serde_json::to_value(rows)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_model_stats" => {
            let rows = ctx.state.db.get_model_stats(
                optional_arg::<i64>(&args, "startDate")?,
                optional_arg::<i64>(&args, "endDate")?,
                optional_arg::<String>(&args, "appType")?.as_deref(),
                optional_arg::<String>(&args, "providerName")?.as_deref(),
                optional_arg::<String>(&args, "model")?.as_deref(),
            )?;
            Ok(serde_json::to_value(rows)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_request_logs" => {
            let filters = arg::<LogFilters>(&args, "filters")?;
            let page = args.get("page").and_then(Value::as_u64).unwrap_or(0) as u32;
            let page_size = args
                .get("pageSize")
                .and_then(Value::as_u64)
                .unwrap_or(20) as u32;
            let logs = ctx.state.db.get_request_logs(&filters, page, page_size)?;
            Ok(serde_json::to_value(logs)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_request_detail" => {
            let request_id = arg::<String>(&args, "requestId")?;
            let detail = ctx.state.db.get_request_detail(&request_id)?;
            Ok(serde_json::to_value(detail)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_model_pricing" => {
            ctx.state.db.ensure_model_pricing_seeded()?;
            crate::services::model_pricing::sync_local_model_pricing(&ctx.state.db)?;
            let conn = crate::database::lock_conn!(ctx.state.db.conn);
            let mut stmt = conn.prepare(
                "SELECT model_id, display_name, input_cost_per_million, output_cost_per_million,
                        cache_read_cost_per_million, cache_creation_cost_per_million
                 FROM model_pricing ORDER BY display_name",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(ModelPricingInfo {
                    model_id: row.get(0)?,
                    display_name: row.get(1)?,
                    input_cost_per_million: row.get(2)?,
                    output_cost_per_million: row.get(3)?,
                    cache_read_cost_per_million: row.get(4)?,
                    cache_creation_cost_per_million: row.get(5)?,
                })
            })?;
            let mut pricing = Vec::new();
            for row in rows {
                pricing.push(row?);
            }
            Ok(serde_json::to_value(pricing)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "update_model_pricing" => {
            crate::services::model_pricing::update_model_pricing(
                &ctx.state.db,
                ModelPricingInfo {
                    model_id: arg::<String>(&args, "modelId")?,
                    display_name: arg::<String>(&args, "displayName")?,
                    input_cost_per_million: arg::<String>(&args, "inputCost")?,
                    output_cost_per_million: arg::<String>(&args, "outputCost")?,
                    cache_read_cost_per_million: arg::<String>(&args, "cacheReadCost")?,
                    cache_creation_cost_per_million: arg::<String>(&args, "cacheCreationCost")?,
                },
            )?;
            Ok(json!(null))
        }
        "update_model_pricing_batch" => {
            let entries = arg::<Vec<ModelPricingInfo>>(&args, "entries")?;
            let count =
                crate::services::model_pricing::update_model_pricing_batch(&ctx.state.db, entries)?;
            Ok(json!(count))
        }
        "get_models_dev_sync_config" => {
            let state = crate::services::model_pricing::get_models_dev_sync_state(&ctx.state.db)?;
            Ok(serde_json::to_value(state)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "save_models_dev_sync_config" => {
            let config = arg::<crate::services::model_pricing::ModelsDevSyncConfig>(
                &args, "config",
            )?;
            crate::services::model_pricing::save_models_dev_sync_config(&ctx.state.db, config)?;
            Ok(json!(null))
        }
        "record_models_dev_sync_result" => {
            let synced_at = optional_arg::<i64>(&args, "syncedAt")?;
            let error = optional_arg::<String>(&args, "error")?;
            crate::services::model_pricing::record_models_dev_sync_result(
                &ctx.state.db,
                synced_at,
                error,
            )?;
            Ok(json!(null))
        }
        "check_provider_limits" => {
            let provider_id = arg::<String>(&args, "providerId")?;
            let app_type = arg::<String>(&args, "appType")?;
            let status = ctx.state.db.check_provider_limits(&provider_id, &app_type)?;
            Ok(serde_json::to_value(status)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "delete_model_pricing" => {
            let model_id = arg::<String>(&args, "modelId")?;
            crate::services::model_pricing::delete_model_pricing(&ctx.state.db, &model_id)?;
            Ok(json!(null))
        }
        "sync_session_usage" => {
            let db = ctx.state.db.clone();
            let _guard = crate::services::session_usage::session_sync_mutex().lock().await;
            let result = tokio::task::spawn_blocking(move || {
                crate::services::session_usage::sync_all_unlocked(&db)
            })
            .await
            .map_err(|e| RpcError::Internal(format!("会话用量同步任务失败: {e}")))?;
            ctx.events.publish("usage-log-recorded", json!({}));
            Ok(serde_json::to_value(result)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_session_usage_last_sync" => Ok(json!(
            crate::services::session_usage::last_sync_completed_at()
        )),
        "rebuild_codex_usage" => {
            let db = ctx.state.db.clone();
            let _guard = crate::services::session_usage::session_sync_mutex().lock().await;
            let result = tokio::task::spawn_blocking(
                move || -> Result<_, crate::error::AppError> {
                    db.backup_database_file()?;
                    db.reset_codex_usage()?;
                    crate::services::session_usage_codex::sync_codex_usage(&db)
                },
            )
            .await
            .map_err(|e| RpcError::Internal(format!("Codex 用量重建任务失败: {e}")))??;
            crate::usage_events::notify_log_recorded();
            ctx.events.publish("usage-log-recorded", json!({}));
            Ok(serde_json::to_value(result)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_usage_data_sources" => {
            let rows = crate::services::session_usage::get_data_source_breakdown(&ctx.state.db)?;
            Ok(serde_json::to_value(rows)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "start_proxy_server" => Ok(serde_json::to_value(
            ctx.state
                .proxy_service
                .start()
                .await
                .map_err(RpcError::Internal)?,
        )
        .map_err(|e| RpcError::Internal(e.to_string()))?),
        "stop_proxy_server" => {
            let takeover = ctx
                .state
                .proxy_service
                .get_takeover_status()
                .await
                .map_err(RpcError::Internal)?;
            if takeover.claude
                || takeover.codex
                || takeover.gemini
                || takeover.grokbuild
                || takeover.claude_desktop
                || takeover.opencode
                || takeover.openclaw
            {
                return Err(RpcError::BadRequest(
                    "仍有应用处于代理接管状态，请先关闭对应应用接管后再停止本地路由。".into(),
                ));
            }
            ctx.state
                .proxy_service
                .stop()
                .await
                .map_err(RpcError::Internal)?;
            Ok(json!(null))
        }
        "stop_proxy_with_restore" => {
            crate::mode::controller::exit_all(&ctx.state)
                .await
                .map_err(RpcError::Internal)?;
            Ok(json!(null))
        }
        "get_proxy_takeover_status" => Ok(serde_json::to_value(
            ctx.state
                .proxy_service
                .get_takeover_status()
                .await
                .map_err(RpcError::Internal)?,
        )
        .map_err(|e| RpcError::Internal(e.to_string()))?),
        "set_proxy_takeover_for_app" => {
            let app_str = arg::<String>(&args, "appType")?;
            let enabled = arg::<bool>(&args, "enabled")?;
            let stack = optional_arg::<bool>(&args, "stack")?.unwrap_or(false);
            let route = optional_arg::<String>(&args, "route")?;
            let app = require_mode_app(&app_str)?;
            if enabled {
                crate::mode::controller::enter_with_route(
                    &ctx.state,
                    &app,
                    stack,
                    route.as_deref(),
                )
                .await
                .map_err(RpcError::Internal)?;
            } else {
                crate::mode::controller::exit(&ctx.state, &app)
                    .await
                    .map_err(RpcError::Internal)?;
            }
            Ok(json!(null))
        }
        "get_app_mode" => {
            let app_str = arg::<String>(&args, "appType")?;
            let app = require_mode_app(&app_str)?;
            let view = crate::mode::controller::app_mode_view(&ctx.state, &app)
                .map_err(RpcError::BadRequest)?;
            Ok(serde_json::to_value(view)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "set_proxy_route" => {
            let app_str = arg::<String>(&args, "appType")?;
            let provider_id = arg::<String>(&args, "providerId")?;
            let app = require_mode_app(&app_str)?;
            let result = crate::mode::controller::set_route(
                &ctx.state,
                &app,
                &provider_id,
            )
            .await
            .map_err(RpcError::Internal)?;
            ctx.events.publish(
                "provider-switched",
                json!({ "appType": app_str, "providerId": provider_id }),
            );
            Ok(serde_json::to_value(result)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "take_startup_attach_failures" => Ok(serde_json::to_value(
            crate::mode::controller::take_startup_attach_failures(),
        )
        .map_err(|e| RpcError::Internal(e.to_string()))?),
        "exit_proxy_apps_in_mode" => {
            let stack = arg::<bool>(&args, "stack")?;
            let apps = crate::mode::controller::exit_apps_in_mode(&ctx.state, stack)
                .await
                .map_err(RpcError::Internal)?;
            Ok(json!(apps))
        }
        "get_direct_provider" => {
            let app_str = arg::<String>(&args, "appType")?;
            let app = require_mode_app(&app_str)?;
            let provider = crate::mode::controller::direct_provider_id(&ctx.state, &app)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(provider))
        }
        "get_proxy_stack" => {
            let app_str = arg::<String>(&args, "appType")?;
            let app = require_mode_app(&app_str)?;
            let view = crate::mode::controller::stack_view_with_clients(&ctx.state, &app)
                .await
                .map_err(RpcError::Internal)?;
            Ok(serde_json::to_value(view)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_proxy_status" => Ok(serde_json::to_value(
            ctx.state
                .proxy_service
                .get_status()
                .await
                .map_err(RpcError::Internal)?,
        )
        .map_err(|e| RpcError::Internal(e.to_string()))?),
        "get_proxy_config" => Ok(serde_json::to_value(
            ctx.state
                .proxy_service
                .get_config()
                .await
                .map_err(RpcError::Internal)?,
        )
        .map_err(|e| RpcError::Internal(e.to_string()))?),
        "update_proxy_config" => {
            let config = arg::<crate::proxy::types::ProxyConfig>(&args, "config")?;
            if ctx
                .state
                .proxy_service
                .update_config(&config)
                .await
                .map_err(RpcError::Internal)?
            {
                crate::mode::controller::resync_routes(&ctx.state)
                    .await
                    .map_err(RpcError::Internal)?;
            }
            Ok(json!(null))
        }
        "get_global_proxy_config" => Ok(serde_json::to_value(
            ctx.state
                .db
                .get_global_proxy_config()
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?,
        )
        .map_err(|e| RpcError::Internal(e.to_string()))?),
        "update_global_proxy_config" => {
            let config = arg::<crate::proxy::types::GlobalProxyConfig>(&args, "config")?;
            let restarted = ctx
                .state
                .proxy_service
                .update_global_config(&config)
                .await
                .map_err(RpcError::Internal)?;
            if restarted {
                crate::mode::controller::resync_routes(&ctx.state)
                    .await
                    .map_err(RpcError::Internal)?;
            }
            Ok(json!(null))
        }
        "get_proxy_client_host" => Ok(json!(
            crate::settings::get_proxy_client_host().unwrap_or_default()
        )),
        "set_proxy_client_host" => {
            let host = arg::<String>(&args, "host")?;
            crate::settings::set_proxy_client_host(&host)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            crate::mode::controller::resync_routes(&ctx.state)
                .await
                .map_err(RpcError::Internal)?;
            Ok(json!(null))
        }
        "get_proxy_config_for_app" => {
            let app_str = arg::<String>(&args, "appType")?;
            require_proxy_app(&app_str)?;
            let config = ctx
                .state
                .db
                .get_proxy_config_for_app(&app_str)
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(config)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "update_proxy_config_for_app" => {
            let mut config = arg::<crate::proxy::types::AppProxyConfig>(&args, "config")?;
            let app = require_proxy_app(&config.app_type)?;
            let circuit_config = crate::proxy::CircuitBreakerConfig::from(&config);
            config.enabled = crate::mode::current::is_proxy(&app);
            let app_type_str = config.app_type.clone();
            ctx.state
                .db
                .update_proxy_config_for_app(config)
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            ctx.state
                .proxy_service
                .update_circuit_breaker_config_for_app(&app_type_str, circuit_config)
                .await
                .map_err(RpcError::Internal)?;
            Ok(json!(null))
        }
        "get_pricing_model_source" => {
            let app_type = arg::<String>(&args, "appType")?;
            let source = ctx
                .state
                .db
                .get_pricing_model_source(&app_type)
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(source))
        }
        "set_pricing_model_source" => {
            let app_type = arg::<String>(&args, "appType")?;
            let value = arg::<String>(&args, "value")?;
            ctx.state
                .db
                .set_pricing_model_source(&app_type, &value)
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(null))
        }
        "is_proxy_running" => Ok(json!(ctx.state.proxy_service.is_running().await)),
        "is_live_takeover_active" => Ok(json!(
            ctx.state
                .proxy_service
                .is_takeover_active()
                .await
                .map_err(RpcError::Internal)?
        )),
        "switch_proxy_provider" => {
            let app_str = arg::<String>(&args, "appType")?;
            let provider_id = arg::<String>(&args, "providerId")?;
            let app = require_mode_app(&app_str)?;
            crate::mode::controller::switch_route(&ctx.state, &app, &provider_id)
                .await
                .map_err(RpcError::Internal)?;
            ctx.events.publish(
                "provider-switched",
                json!({ "appType": app_str, "providerId": provider_id }),
            );
            Ok(json!(null))
        }
        "get_provider_health" => {
            let provider_id = arg::<String>(&args, "providerId")?;
            let app_type = arg::<String>(&args, "appType")?;
            require_mode_app(&app_type)?;
            let health = ctx
                .state
                .db
                .get_provider_health(&provider_id, &app_type)
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(health)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "reset_circuit_breaker" => {
            let provider_id = arg::<String>(&args, "providerId")?;
            let app_type = arg::<String>(&args, "appType")?;
            require_proxy_app(&app_type)?;
            ctx.state
                .db
                .update_provider_health(&provider_id, &app_type, true, None)
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            ctx.state
                .proxy_service
                .reset_provider_circuit_breaker(&provider_id, &app_type)
                .await
                .map_err(RpcError::Internal)?;
            ctx.events.publish(
                "provider-health-updated",
                json!({ "providerId": provider_id, "appType": app_type }),
            );
            Ok(json!(null))
        }
        "get_circuit_breaker_config" => Ok(serde_json::to_value(
            ctx.state
                .db
                .get_circuit_breaker_config()
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?,
        )
        .map_err(|e| RpcError::Internal(e.to_string()))?),
        "update_circuit_breaker_config" => {
            let config = arg::<crate::proxy::CircuitBreakerConfig>(&args, "config")?;
            ctx.state
                .db
                .update_circuit_breaker_config(&config)
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            ctx.state
                .proxy_service
                .update_circuit_breaker_configs(config)
                .await
                .map_err(RpcError::Internal)?;
            Ok(json!(null))
        }
        "get_circuit_breaker_stats" => Ok(json!(null)),
        "restart_codex_app_server_daemon" => {
            let outcome = crate::services::provider::codex_direct::off_runtime(
                crate::services::provider::codex_client_catalog::restart_daemon,
            )
            .await
            .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(outcome)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "acknowledge_codex_stale_clients" => {
            crate::services::provider::codex_direct::off_runtime(|| {
                crate::services::provider::codex_client_catalog::acknowledge(
                    &crate::live::engine::DeviceStore::for_device(),
                )
            })
            .await
            .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(null))
        }
        "adopt_codex_stack_catalog" => {
            let hint = crate::mode::controller::adopt_codex_stack_catalog(&ctx.state)
                .await
                .map_err(RpcError::Internal)?;
            Ok(json!(hint))
        }
        "get_failover_queue" => {
            let app_type = arg::<String>(&args, "appType")?;
            require_proxy_app(&app_type)?;
            let queue = ctx
                .state
                .db
                .get_failover_queue(&app_type)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(queue)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_available_providers_for_failover" => {
            let app_type = arg::<String>(&args, "appType")?;
            require_proxy_app(&app_type)?;
            let providers = ctx
                .state
                .db
                .get_available_providers_for_failover(&app_type)
                .map_err(|e| RpcError::Internal(e.to_string()))?
                .into_iter()
                .filter(|provider| {
                    crate::proxy::provider_router::provider_supports_failover(
                        &app_type, provider,
                    )
                })
                .collect::<Vec<_>>();
            Ok(serde_json::to_value(providers)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "add_to_failover_queue" => {
            let app_type = arg::<String>(&args, "appType")?;
            let provider_id = arg::<String>(&args, "providerId")?;
            require_proxy_app(&app_type)?;
            let provider = ctx
                .state
                .db
                .get_provider_by_id(&provider_id, &app_type)
                .map_err(|e| RpcError::Internal(e.to_string()))?
                .ok_or_else(|| RpcError::BadRequest(format!("供应商不存在: {provider_id}")))?;
            if !crate::proxy::provider_router::provider_supports_failover(
                &app_type, &provider,
            ) {
                return Err(RpcError::BadRequest(
                    "该供应商不支持自动故障转移".into(),
                ));
            }
            ctx.state
                .db
                .add_to_failover_queue(&app_type, &provider_id)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(null))
        }
        "remove_from_failover_queue" => {
            let app_type = arg::<String>(&args, "appType")?;
            let provider_id = arg::<String>(&args, "providerId")?;
            require_proxy_app(&app_type)?;
            ctx.state
                .db
                .remove_from_failover_queue(&app_type, &provider_id)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(null))
        }
        "get_auto_failover_enabled" => {
            let app_type = arg::<String>(&args, "appType")?;
            require_proxy_app(&app_type)?;
            let config = ctx
                .state
                .db
                .get_proxy_config_for_app(&app_type)
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(config.auto_failover_enabled))
        }
        "set_auto_failover_enabled" => {
            let app_str = arg::<String>(&args, "appType")?;
            let enabled = arg::<bool>(&args, "enabled")?;
            require_proxy_app(&app_str)?;
            let app = app_type(&app_str)?;
            let mut config = ctx
                .state
                .db
                .get_proxy_config_for_app(&app_str)
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            if enabled && !crate::mode::current::is_proxy(&app) {
                return Err(RpcError::BadRequest(
                    "需要先让该应用进入路由模式，再开启故障转移".into(),
                ));
            }
            let mut auto_added: Option<String> = None;
            let p1_provider_id = if enabled {
                let all_providers = ctx
                    .state
                    .db
                    .get_all_providers(&app_str)
                    .map_err(|e| RpcError::Internal(e.to_string()))?;
                let mut queue = ctx
                    .state
                    .db
                    .get_failover_queue(&app_str)
                    .map_err(|e| RpcError::Internal(e.to_string()))?
                    .into_iter()
                    .filter(|item| {
                        all_providers.get(&item.provider_id).is_some_and(|provider| {
                            crate::proxy::provider_router::provider_supports_failover(
                                &app_str, provider,
                            )
                        })
                    })
                    .collect::<Vec<_>>();
                if queue.is_empty() {
                    let current_id = crate::mode::current::provider_for(
                        &ctx.state.db,
                        &app,
                        crate::mode::current::Purpose::InUse,
                    )
                    .map_err(|e| RpcError::Internal(e.to_string()))?
                    .ok_or_else(|| {
                        RpcError::BadRequest(
                            "故障转移队列为空，且未设置当前供应商，无法开启故障转移".into(),
                        )
                    })?;
                    ctx.state
                        .db
                        .add_to_failover_queue(&app_str, &current_id)
                        .map_err(|e| RpcError::Internal(e.to_string()))?;
                    auto_added = Some(current_id);
                    queue = ctx
                        .state
                        .db
                        .get_failover_queue(&app_str)
                        .map_err(|e| RpcError::Internal(e.to_string()))?
                        .into_iter()
                        .filter(|item| {
                            all_providers.get(&item.provider_id).is_some_and(|provider| {
                                crate::proxy::provider_router::provider_supports_failover(
                                    &app_str, provider,
                                )
                            })
                        })
                        .collect();
                }
                queue
                    .first()
                    .map(|item| item.provider_id.clone())
                    .ok_or_else(|| {
                        RpcError::BadRequest("故障转移队列为空，无法开启故障转移".into())
                    })?
            } else {
                String::new()
            };

            if enabled {
                if let Err(error) = crate::mode::controller::switch_route_for_failover(
                    &ctx.state,
                    &app,
                    &p1_provider_id,
                )
                .await
                {
                    if let Some(provider_id) = auto_added {
                        let _ = ctx
                            .state
                            .db
                            .remove_from_failover_queue(&app_str, &provider_id);
                    }
                    return Err(RpcError::Internal(error));
                }
            }

            config.auto_failover_enabled = enabled;
            config.enabled = crate::mode::current::is_proxy(&app);
            ctx.state
                .db
                .update_proxy_config_for_app(config)
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;

            if enabled {
                ctx.events.publish(
                    "provider-switched",
                    json!({
                        "appType": app_str,
                        "providerId": p1_provider_id,
                        "source": "failoverEnabled"
                    }),
                );
            }
            Ok(json!(null))
        }
        "list_sessions" => {
            let sessions = tokio::task::spawn_blocking(crate::session_manager::scan_sessions)
                .await
                .map_err(|e| RpcError::Internal(format!("扫描会话失败: {e}")))?;
            Ok(serde_json::to_value(sessions)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_session_messages" => {
            let provider_id = arg::<String>(&args, "providerId")?;
            let source_path = arg::<String>(&args, "sourcePath")?;
            let messages = tokio::task::spawn_blocking(move || {
                crate::session_manager::load_transcript(&provider_id, &source_path)
                    .map(|loaded| loaded.transcript.messages.clone())
            })
            .await
            .map_err(|e| RpcError::Internal(format!("读取会话消息失败: {e}")))??;
            Ok(serde_json::to_value(messages)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_session_block_content" => {
            let provider_id = arg::<String>(&args, "providerId")?;
            let source_path = arg::<String>(&args, "sourcePath")?;
            let content_ref = arg::<crate::session_manager::model::ContentRef>(
                &args,
                "contentRef",
            )?;
            let offset = optional_arg::<u32>(&args, "offset")?;
            let limit = optional_arg::<u32>(&args, "limit")?;
            let block = tokio::task::spawn_blocking(move || {
                let source = crate::session_manager::content::validate_source(
                    &provider_id,
                    &source_path,
                )?;
                let full =
                    crate::session_manager::content::resolve_content_ref(&source, &content_ref)?;
                Ok::<_, String>(crate::session_manager::content::paginate(
                    &full, offset, limit,
                ))
            })
            .await
            .map_err(|e| RpcError::Internal(format!("读取会话内容失败: {e}")))??;
            Ok(serde_json::to_value(block)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "delete_session" => {
            let provider_id = arg::<String>(&args, "providerId")?;
            let session_id = arg::<String>(&args, "sessionId")?;
            let source_path = arg::<String>(&args, "sourcePath")?;
            let deleted = tokio::task::spawn_blocking(move || {
                crate::session_manager::delete_session(&provider_id, &session_id, &source_path)
            })
            .await
            .map_err(|e| RpcError::Internal(format!("删除会话失败: {e}")))??;
            Ok(json!(deleted))
        }
        "delete_sessions" => {
            let items = arg::<Vec<crate::session_manager::DeleteSessionRequest>>(
                &args, "items",
            )?;
            let outcomes = tokio::task::spawn_blocking(move || {
                crate::session_manager::delete_sessions(&items)
            })
            .await
            .map_err(|e| RpcError::Internal(format!("批量删除会话失败: {e}")))?;
            Ok(serde_json::to_value(outcomes)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "has_codex_unify_history_backup" => Ok(json!(
            crate::codex_history_migration::has_codex_official_history_unify_backup()
        )),
        "codex_forces_multi_agent_v2" => {
            let text = crate::codex_config::read_codex_config_text()
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(crate::codex_config::codex_config_forces_multi_agent_v2(
                &text
            )))
        }
        "get_codex_session_compression" => Ok(json!(
            crate::services::codex_session_compression::is_enabled()
                .map_err(|e| RpcError::Internal(e.to_string()))?
        )),
        "set_codex_session_compression" => {
            let enabled = arg::<bool>(&args, "enabled")?;
            let db = ctx.state.db.clone();
            let result = tokio::task::spawn_blocking(move || {
                crate::services::codex_session_compression::set_enabled(&db, enabled)
            })
            .await
            .map_err(|e| RpcError::Internal(format!("设置会话压缩失败: {e}")))??;
            Ok(json!(result))
        }
        "get_codex_sessions_disk_usage" => {
            let usage = tokio::task::spawn_blocking(
                crate::services::codex_session_compression::sessions_disk_usage,
            )
            .await
            .map_err(|e| RpcError::Internal(format!("读取会话磁盘占用失败: {e}")))?;
            Ok(json!(usage))
        }
        "restore_codex_unified_history" => {
            let outcome = tokio::task::spawn_blocking(|| {
                crate::codex_history_migration::restore_codex_official_history_from_backups()
            })
            .await
            .map_err(|e| RpcError::Internal(format!("恢复统一会话历史失败: {e}")))??;
            Ok(json!({
                "restoredJsonlFiles": outcome.restored_jsonl_files,
                "restoredStateRows": outcome.restored_state_rows,
                "skippedReason": outcome.skipped_reason,
            }))
        }
        "get_rectifier_config" => Ok(serde_json::to_value(
            ctx.state
                .db
                .get_rectifier_config()
                .map_err(|e| RpcError::Internal(e.to_string()))?,
        )
        .map_err(|e| RpcError::Internal(e.to_string()))?),
        "set_rectifier_config" => {
            let config = arg::<crate::proxy::types::RectifierConfig>(&args, "config")?;
            ctx.state
                .db
                .set_rectifier_config(&config)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(true))
        }
        "get_optimizer_config" => Ok(serde_json::to_value(
            ctx.state
                .db
                .get_optimizer_config()
                .map_err(|e| RpcError::Internal(e.to_string()))?,
        )
        .map_err(|e| RpcError::Internal(e.to_string()))?),
        "set_optimizer_config" => {
            let config = arg::<crate::proxy::types::OptimizerConfig>(&args, "config")?;
            ctx.state
                .db
                .set_optimizer_config(&config)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(true))
        }
        "get_copilot_optimizer_config" => Ok(serde_json::to_value(
            ctx.state
                .db
                .get_copilot_optimizer_config()
                .map_err(|e| RpcError::Internal(e.to_string()))?,
        )
        .map_err(|e| RpcError::Internal(e.to_string()))?),
        "set_copilot_optimizer_config" => {
            let config =
                arg::<crate::proxy::types::CopilotOptimizerConfig>(&args, "config")?;
            ctx.state
                .db
                .set_copilot_optimizer_config(&config)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(true))
        }
        "get_log_config" => Ok(serde_json::to_value(
            ctx.state
                .db
                .get_log_config()
                .map_err(|e| RpcError::Internal(e.to_string()))?,
        )
        .map_err(|e| RpcError::Internal(e.to_string()))?),
        "set_log_config" => {
            let config = arg::<crate::proxy::types::LogConfig>(&args, "config")?;
            ctx.state
                .db
                .set_log_config(&config)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            log::set_max_level(config.to_level_filter());
            Ok(json!(true))
        }
        "auth_get_status" | "auth_list_accounts" => {
            let provider = arg::<String>(&args, "authProvider")?;
            ensure_auth_provider(&provider)?;
            let (authenticated, default_account_id, migration_error, accounts) =
                auth_snapshot(ctx, &provider).await?;
            if command == "auth_list_accounts" {
                Ok(json!(accounts))
            } else {
                Ok(json!({
                    "provider": provider,
                    "authenticated": authenticated,
                    "defaultAccountId": default_account_id,
                    "migrationError": migration_error,
                    "accounts": accounts,
                }))
            }
        }
        "auth_start_login" => {
            let provider = arg::<String>(&args, "authProvider")?;
            ensure_auth_provider(&provider)?;
            let github_domain = optional_arg::<String>(&args, "githubDomain")?;
            let target_account_id = optional_arg::<String>(&args, "targetAccountId")?;
            let response = match provider.as_str() {
                AUTH_COPILOT => {
                    if target_account_id.is_some() {
                        return Err(RpcError::BadRequest(
                            "定向重新登录仅支持 Codex OAuth".into(),
                        ));
                    }
                    ctx.copilot
                        .read()
                        .await
                        .start_device_flow(github_domain.as_deref())
                        .await
                        .map_err(|e| RpcError::Internal(e.to_string()))?
                }
                AUTH_CODEX => ctx
                    .state
                    .codex_oauth_manager
                    .start_device_flow(target_account_id.as_deref())
                    .await
                    .map_err(|e| RpcError::Internal(e.to_string()))?,
                AUTH_XAI => {
                    if target_account_id.is_some() {
                        return Err(RpcError::BadRequest(
                            "定向重新登录仅支持 Codex OAuth".into(),
                        ));
                    }
                    ctx.xai
                        .read()
                        .await
                        .start_device_flow()
                        .await
                        .map_err(|e| RpcError::Internal(e.to_string()))?
                }
                _ => unreachable!(),
            };
            Ok(json!({
                "provider": provider,
                "deviceCode": response.device_code,
                "userCode": response.user_code,
                "verificationUri": response.verification_uri,
                "expiresIn": response.expires_in,
                "interval": response.interval,
            }))
        }
        "auth_poll_for_account" => {
            let provider = arg::<String>(&args, "authProvider")?;
            ensure_auth_provider(&provider)?;
            let device_code = arg::<String>(&args, "deviceCode")?;
            let github_domain = optional_arg::<String>(&args, "githubDomain")?;
            match provider.as_str() {
                AUTH_COPILOT => {
                    let manager = ctx.copilot.write().await;
                    match manager
                        .poll_for_token(&device_code, github_domain.as_deref())
                        .await
                    {
                        Ok(account) => {
                            let default_id =
                                manager.get_status().await.default_account_id;
                            Ok(json!(account.map(|account| {
                                account_json(&provider, account, default_id.as_deref())
                            })))
                        }
                        Err(crate::proxy::providers::copilot_auth::CopilotAuthError::AuthorizationPending) => {
                            Ok(json!(null))
                        }
                        Err(e) => Err(RpcError::Internal(e.to_string())),
                    }
                }
                AUTH_CODEX => {
                    let manager = &ctx.state.codex_oauth_manager;
                    let proxy = ctx.state.proxy_service.clone();
                    match manager
                        .poll_for_token(&device_code, || async {
                            proxy.lock_switch_for_app(AppType::Codex.as_str()).await
                        })
                        .await
                    {
                        Ok(account) => {
                            let default_id =
                                manager.get_status().await.default_account_id;
                            Ok(json!(account.map(|account| {
                                account_json(&provider, account, default_id.as_deref())
                            })))
                        }
                        Err(crate::proxy::providers::codex_oauth_auth::CodexOAuthError::AuthorizationPending) => {
                            Ok(json!(null))
                        }
                        Err(e) => Err(RpcError::Internal(e.to_string())),
                    }
                }
                AUTH_XAI => {
                    let manager = ctx.xai.write().await;
                    match manager.poll_for_token(&device_code).await {
                        Ok(account) => {
                            let default_id =
                                manager.get_status().await.default_account_id;
                            Ok(json!(account.map(|account| {
                                xai_account_json(&provider, account, default_id.as_deref())
                            })))
                        }
                        Err(crate::proxy::providers::xai_oauth_auth::XaiOAuthError::AuthorizationPending) => {
                            Ok(json!(null))
                        }
                        Err(e) => Err(RpcError::Internal(e.to_string())),
                    }
                }
                _ => unreachable!(),
            }
        }
        "auth_cancel_login" => {
            let provider = arg::<String>(&args, "authProvider")?;
            ensure_auth_provider(&provider)?;
            if provider != AUTH_CODEX {
                return Err(RpcError::BadRequest(
                    "登录取消仅支持 Codex OAuth".into(),
                ));
            }
            let device_code = arg::<String>(&args, "deviceCode")?;
            Ok(json!(
                ctx.state
                    .codex_oauth_manager
                    .cancel_device_flow(&device_code)
                    .await
            ))
        }
        "auth_remove_account" => {
            let provider = arg::<String>(&args, "authProvider")?;
            ensure_auth_provider(&provider)?;
            let account_id = arg::<String>(&args, "accountId")?;
            match provider.as_str() {
                AUTH_COPILOT => {
                    ctx.copilot
                        .write()
                        .await
                        .remove_account(&account_id)
                        .await
                        .map_err(|e| RpcError::Internal(e.to_string()))?;
                }
                AUTH_CODEX => {
                    let _guard = ctx
                        .state
                        .proxy_service
                        .lock_switch_for_app(AppType::Codex.as_str())
                        .await;
                    ctx.state
                        .codex_oauth_manager
                        .remove_account(&account_id)
                        .await
                        .map_err(|e| RpcError::Internal(e.to_string()))?;
                    ctx.state
                        .db
                        .unbind_codex_managed_accounts(&[account_id.clone()])
                        .map_err(|e| RpcError::Internal(e.to_string()))?;
                }
                AUTH_XAI => {
                    ctx.xai
                        .write()
                        .await
                        .remove_account(&account_id)
                        .await
                        .map_err(|e| RpcError::Internal(e.to_string()))?;
                }
                _ => unreachable!(),
            }
            Ok(json!(null))
        }
        "auth_set_default_account" => {
            let provider = arg::<String>(&args, "authProvider")?;
            ensure_auth_provider(&provider)?;
            let account_id = arg::<String>(&args, "accountId")?;
            match provider.as_str() {
                AUTH_COPILOT => {
                    ctx.copilot
                        .write()
                        .await
                        .set_default_account(&account_id)
                        .await
                        .map_err(|e| RpcError::Internal(e.to_string()))?;
                }
                AUTH_CODEX => {
                    ctx.state
                        .codex_oauth_manager
                        .set_default_account(&account_id)
                        .await
                        .map_err(|e| RpcError::Internal(e.to_string()))?;
                }
                AUTH_XAI => {
                    ctx.xai
                        .write()
                        .await
                        .set_default_account(&account_id)
                        .await
                        .map_err(|e| RpcError::Internal(e.to_string()))?;
                }
                _ => unreachable!(),
            }
            Ok(json!(null))
        }
        "auth_logout" => {
            let provider = arg::<String>(&args, "authProvider")?;
            ensure_auth_provider(&provider)?;
            match provider.as_str() {
                AUTH_COPILOT => {
                    ctx.copilot
                        .write()
                        .await
                        .clear_auth()
                        .await
                        .map_err(|e| RpcError::Internal(e.to_string()))?;
                }
                AUTH_CODEX => {
                    let _guard = ctx
                        .state
                        .proxy_service
                        .lock_switch_for_app(AppType::Codex.as_str())
                        .await;
                    let removed: Vec<String> = ctx
                        .state
                        .codex_oauth_manager
                        .list_accounts()
                        .await
                        .into_iter()
                        .map(|account| account.id)
                        .collect();
                    ctx.state
                        .codex_oauth_manager
                        .clear_auth()
                        .await
                        .map_err(|e| RpcError::Internal(e.to_string()))?;
                    ctx.state
                        .db
                        .unbind_codex_managed_accounts(&removed)
                        .map_err(|e| RpcError::Internal(e.to_string()))?;
                }
                AUTH_XAI => {
                    ctx.xai
                        .write()
                        .await
                        .clear_auth()
                        .await
                        .map_err(|e| RpcError::Internal(e.to_string()))?;
                }
                _ => unreachable!(),
            }
            Ok(json!(null))
        }
        "copilot_start_device_flow" => {
            let domain = optional_arg::<String>(&args, "githubDomain")?;
            let response = ctx
                .copilot
                .read()
                .await
                .start_device_flow(domain.as_deref())
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(response)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "copilot_poll_for_auth" | "copilot_poll_for_account" => {
            let device_code = arg::<String>(&args, "deviceCode")?;
            let domain = optional_arg::<String>(&args, "githubDomain")?;
            let manager = ctx.copilot.write().await;
            match manager
                .poll_for_token(&device_code, domain.as_deref())
                .await
            {
                Ok(account) => {
                    if command == "copilot_poll_for_auth" {
                        Ok(json!(account.is_some()))
                    } else {
                        Ok(serde_json::to_value(account)
                            .map_err(|e| RpcError::Internal(e.to_string()))?)
                    }
                }
                Err(crate::proxy::providers::copilot_auth::CopilotAuthError::AuthorizationPending) => {
                    Ok(if command == "copilot_poll_for_auth" {
                        json!(false)
                    } else {
                        json!(null)
                    })
                }
                Err(e) => Err(RpcError::Internal(e.to_string())),
            }
        }
        "copilot_list_accounts" => {
            let accounts = ctx.copilot.read().await.list_accounts().await;
            Ok(serde_json::to_value(accounts)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "copilot_remove_account" => {
            let account_id = arg::<String>(&args, "accountId")?;
            ctx.copilot
                .write()
                .await
                .remove_account(&account_id)
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(null))
        }
        "copilot_set_default_account" => {
            let account_id = arg::<String>(&args, "accountId")?;
            ctx.copilot
                .write()
                .await
                .set_default_account(&account_id)
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(null))
        }
        "copilot_get_auth_status" => {
            let status = ctx.copilot.read().await.get_status().await;
            Ok(serde_json::to_value(status)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "copilot_is_authenticated" => Ok(json!(
            ctx.copilot.read().await.is_authenticated().await
        )),
        "copilot_logout" => {
            ctx.copilot
                .write()
                .await
                .clear_auth()
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(null))
        }
        "copilot_get_token" => {
            let token = ctx
                .copilot
                .read()
                .await
                .get_valid_token()
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(token))
        }
        "copilot_get_token_for_account" => {
            let account_id = arg::<String>(&args, "accountId")?;
            let token = ctx
                .copilot
                .read()
                .await
                .get_valid_token_for_account(&account_id)
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(token))
        }
        "copilot_get_models" => {
            let models = ctx
                .copilot
                .read()
                .await
                .fetch_models()
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(models)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "copilot_get_models_for_account" => {
            let account_id = arg::<String>(&args, "accountId")?;
            let models = ctx
                .copilot
                .read()
                .await
                .fetch_models_for_account(&account_id)
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(models)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "copilot_get_usage" => {
            let usage = ctx
                .copilot
                .read()
                .await
                .fetch_usage()
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(usage)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "copilot_get_usage_for_account" => {
            let account_id = arg::<String>(&args, "accountId")?;
            let usage = ctx
                .copilot
                .read()
                .await
                .fetch_usage_for_account(&account_id)
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(usage)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "list_daily_memory_files" => Ok(serde_json::to_value(
            crate::commands::list_daily_memory_files()
                .await
                .map_err(RpcError::Internal)?,
        )
        .map_err(|e| RpcError::Internal(e.to_string()))?),
        "read_daily_memory_file" => {
            let filename = arg::<String>(&args, "filename")?;
            Ok(json!(
                crate::commands::read_daily_memory_file(filename)
                    .await
                    .map_err(RpcError::Internal)?
            ))
        }
        "write_daily_memory_file" => {
            let filename = arg::<String>(&args, "filename")?;
            let content = arg::<String>(&args, "content")?;
            crate::commands::write_daily_memory_file(filename, content)
                .await
                .map_err(RpcError::Internal)?;
            Ok(json!(null))
        }
        "search_daily_memory_files" => {
            let query = arg::<String>(&args, "query")?;
            let results = crate::commands::search_daily_memory_files(query)
                .await
                .map_err(RpcError::Internal)?;
            Ok(serde_json::to_value(results)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "delete_daily_memory_file" => {
            let filename = arg::<String>(&args, "filename")?;
            crate::commands::delete_daily_memory_file(filename)
                .await
                .map_err(RpcError::Internal)?;
            Ok(json!(null))
        }
        "read_workspace_file" => {
            let filename = arg::<String>(&args, "filename")?;
            Ok(json!(
                crate::commands::read_workspace_file(filename)
                    .await
                    .map_err(RpcError::Internal)?
            ))
        }
        "write_workspace_file" => {
            let filename = arg::<String>(&args, "filename")?;
            let content = arg::<String>(&args, "content")?;
            crate::commands::write_workspace_file(filename, content)
                .await
                .map_err(RpcError::Internal)?;
            Ok(json!(null))
        }
        "parse_deeplink" => {
            let url = arg::<String>(&args, "url")?;
            let request = crate::commands::parse_deeplink(url)
                .map_err(RpcError::BadRequest)?;
            Ok(serde_json::to_value(request)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "merge_deeplink_config" => {
            let request = arg::<crate::deeplink::DeepLinkImportRequest>(&args, "request")?;
            let merged = crate::commands::merge_deeplink_config(request)
                .map_err(RpcError::BadRequest)?;
            Ok(serde_json::to_value(merged)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "fetch_deeplink_icon" => {
            let url = arg::<String>(&args, "url")?;
            Ok(json!(crate::commands::fetch_deeplink_icon(url).await))
        }
        "import_from_deeplink_unified" => {
            let request = arg::<crate::deeplink::DeepLinkImportRequest>(&args, "request")?;
            import_deeplink_unified(ctx, request).await
        }
        "create_db_backup" => {
            let db = ctx.state.db.clone();
            let filename = tokio::task::spawn_blocking(move || match db.backup_database_file() {
                Ok(Some(path)) => Ok(path
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default()),
                Ok(None) => Err("数据库文件不存在，已跳过备份".to_string()),
                Err(e) => Err(e.to_string()),
            })
            .await
            .map_err(|e| RpcError::Internal(format!("备份失败: {e}")))??;
            Ok(json!(filename))
        }
        "list_db_backups" => {
            let backups = crate::database::Database::list_backups()
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(backups)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "restore_db_backup" => {
            let filename = arg::<String>(&args, "filename")?;
            let db = ctx.state.db.clone();
            let _sync_guard = crate::services::sync_protocol::sync_mutex().lock().await;
            let restored = tokio::task::spawn_blocking(move || {
                db.restore_from_backup(&filename)
            })
            .await
            .map_err(|e| RpcError::Internal(format!("恢复备份失败: {e}")))??;
            Ok(json!(restored))
        }
        "rename_db_backup" => {
            let old_filename = arg::<String>(&args, "oldFilename")?;
            let new_name = arg::<String>(&args, "newName")?;
            let renamed = crate::database::Database::rename_backup(&old_filename, &new_name)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(renamed))
        }
        "delete_db_backup" => {
            let filename = arg::<String>(&args, "filename")?;
            crate::database::Database::delete_backup(&filename)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(null))
        }
        "list_backup_locations" => {
            let locations = tokio::task::spawn_blocking(
                crate::services::backup_storage::list_locations,
            )
            .await
            .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(locations)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "delete_backup_location" => {
            let id = arg::<String>(&args, "id")?;
            let freed = tokio::task::spawn_blocking(move || {
                crate::services::backup_storage::delete_location(&id)
            })
            .await
            .map_err(|e| RpcError::Internal(e.to_string()))??;
            Ok(json!(freed))
        }
        "export_config_to_file" => {
            let file_path = arg::<String>(&args, "filePath")?;
            let path_for_job = file_path.clone();
            let db = ctx.state.db.clone();
            let result = tokio::task::spawn_blocking(move || {
                db.export_sql(std::path::Path::new(&path_for_job))
            })
            .await
            .map_err(|e| RpcError::Internal(format!("导出配置失败: {e}")))??;
            let _ = result;
            Ok(json!({
                "success": true,
                "message": "SQL exported successfully",
                "filePath": file_path,
            }))
        }
        "import_config_from_file" => {
            let file_path = arg::<String>(&args, "filePath")?;
            let db = ctx.state.db.clone();
            let _sync_guard = crate::services::sync_protocol::sync_mutex().lock().await;
            let backup_id = tokio::task::spawn_blocking(move || {
                db.import_sql(std::path::Path::new(&file_path))
            })
            .await
            .map_err(|e| RpcError::Internal(format!("导入配置失败: {e}")))??;
            Ok(json!({
                "success": true,
                "message": "SQL imported successfully",
                "backupId": backup_id,
            }))
        }
        "sync_current_providers_live" => {
            crate::services::ProviderService::sync_current_to_live(&ctx.state)?;
            Ok(json!({
                "success": true,
                "message": "Live configuration synchronized",
            }))
        }
        "get_balance" => {
            let base_url = arg::<String>(&args, "baseUrl")?;
            let api_key = arg::<String>(&args, "apiKey")?;
            let usage = crate::services::balance::get_balance(&base_url, &api_key).await?;
            Ok(serde_json::to_value(usage)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_coding_plan_quota" => {
            let base_url = arg::<String>(&args, "baseUrl")?;
            let api_key = arg::<String>(&args, "apiKey")?;
            let access_key_id = optional_arg::<String>(&args, "accessKeyId")?;
            let secret_access_key = optional_arg::<String>(&args, "secretAccessKey")?;
            let coding_plan_provider =
                optional_arg::<String>(&args, "codingPlanProvider")?;
            let team_organization_id =
                optional_arg::<String>(&args, "teamOrganizationId")?;
            let team_project_id = optional_arg::<String>(&args, "teamProjectId")?;
            let quota = crate::services::coding_plan::get_coding_plan_quota(
                &base_url,
                &api_key,
                access_key_id.as_deref(),
                secret_access_key.as_deref(),
                coding_plan_provider.as_deref(),
                team_organization_id.as_deref(),
                team_project_id.as_deref(),
            )
            .await
            .map_err(RpcError::Internal)?;
            Ok(serde_json::to_value(quota)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_subscription_quota" => {
            let tool = arg::<String>(&args, "tool")?;
            let quota = crate::services::subscription::get_subscription_quota(&tool)
                .await
                .map_err(RpcError::Internal)?;
            if let Ok(app_type) = AppType::from_str(&tool) {
                let app_str = app_type.as_str().to_string();
                ctx.state
                    .usage_cache
                    .put_subscription(app_type, quota.clone());
                ctx.events.publish(
                    "usage-cache-updated",
                    json!({
                        "kind": "subscription",
                        "appType": app_str,
                        "data": quota,
                    }),
                );
            }
            Ok(serde_json::to_value(quota)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_codex_oauth_quota" => {
            let account_id = optional_arg::<String>(&args, "accountId")?;
            let manager = &ctx.state.codex_oauth_manager;
            let resolved = match account_id {
                Some(id) => Some(id.trim().to_string()),
                None => manager.default_account_id().await,
            };
            let Some(id) = resolved else {
                return Ok(serde_json::to_value(
                    crate::services::subscription::SubscriptionQuota::not_found(
                        "codex_oauth",
                    ),
                )
                .map_err(|e| RpcError::Internal(e.to_string()))?);
            };
            let token = match manager.get_valid_token_for_account(&id).await {
                Ok(token) => token,
                Err(e) => {
                    return Ok(serde_json::to_value(
                        crate::services::subscription::SubscriptionQuota::error(
                            "codex_oauth",
                            crate::services::subscription::CredentialStatus::Expired,
                            format!("Codex OAuth token unavailable: {e}"),
                        ),
                    )
                    .map_err(|e| RpcError::Internal(e.to_string()))?)
                }
            };
            let chatgpt_account_id = manager
                .chatgpt_account_id_for_account(&id)
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            let quota = crate::services::subscription::query_codex_quota(
                &token,
                Some(&chatgpt_account_id),
                "codex_oauth",
                "Codex OAuth access token expired or rejected. Please re-login via cc-switch.",
            )
            .await
            .map_err(RpcError::Internal)?;
            ctx.events.publish(
                "usage-cache-updated",
                json!({ "kind": "codex_oauth", "accountId": id, "data": quota }),
            );
            Ok(serde_json::to_value(quota)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_codex_oauth_models" => {
            let account_id = optional_arg::<String>(&args, "accountId")?;
            let manager = &ctx.state.codex_oauth_manager;
            let resolved = match account_id
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty())
            {
                Some(id) => Some(id.to_string()),
                None => manager.default_account_id().await,
            };
            let id = resolved.ok_or_else(|| {
                RpcError::BadRequest("没有可用的 ChatGPT 账号".into())
            })?;
            let token = manager
                .get_valid_token_for_account(&id)
                .await
                .map_err(|e| RpcError::Internal(format!("Codex OAuth token unavailable: {e}")))?;
            let chatgpt_account_id = manager
                .chatgpt_account_id_for_account(&id)
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            let models = crate::services::codex_oauth_models::fetch_models_with_token(
                &token,
                &chatgpt_account_id,
            )
            .await
            .map_err(RpcError::Internal)?;
            Ok(serde_json::to_value(models)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_xai_oauth_quota" => {
            let account_id = optional_arg::<String>(&args, "accountId")?;
            let manager = ctx.xai.read().await;
            let resolved = match account_id
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty())
            {
                Some(id) => Some(id.to_string()),
                None => manager.default_account_id().await,
            };
            let Some(id) = resolved else {
                return Ok(serde_json::to_value(
                    crate::services::subscription::SubscriptionQuota::not_found(
                        "xai_oauth",
                    ),
                )
                .map_err(|e| RpcError::Internal(e.to_string()))?);
            };
            let token = match manager.get_valid_token_for_account(&id).await {
                Ok(token) => token,
                Err(e) => {
                    return Ok(serde_json::to_value(
                        crate::services::subscription::SubscriptionQuota::error(
                            "xai_oauth",
                            crate::services::subscription::CredentialStatus::Expired,
                            format!("xAI OAuth token unavailable: {e}"),
                        ),
                    )
                    .map_err(|e| RpcError::Internal(e.to_string()))?)
                }
            };
            let quota = crate::services::subscription_grok::query_grok_quota(
                &token,
                "xai_oauth",
                "Please re-login via cc-switch.",
            )
            .await
            .map_err(RpcError::Internal)?;
            Ok(serde_json::to_value(quota)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_xai_oauth_models" => {
            let account_id = optional_arg::<String>(&args, "accountId")?;
            let manager = ctx.xai.read().await;
            let resolved = match account_id
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty())
            {
                Some(id) => Some(id.to_string()),
                None => manager.default_account_id().await,
            };
            let id = resolved.ok_or_else(|| {
                RpcError::BadRequest("没有可用的 xAI 账号".into())
            })?;
            let token = manager
                .get_valid_token_for_account(&id)
                .await
                .map_err(|e| RpcError::Internal(format!("xAI OAuth token unavailable: {e}")))?;
            let response = crate::proxy::http_client::get()
                .get(format!("{}/models", crate::proxy::providers::XAI_API_BASE_URL))
                .bearer_auth(token)
                .timeout(std::time::Duration::from_secs(15))
                .send()
                .await
                .map_err(|e| RpcError::Internal(format!("xAI models request failed: {e}")))?;
            if !response.status().is_success() {
                return Err(RpcError::Internal(format!(
                    "xAI models request failed: HTTP {}",
                    response.status()
                )));
            }
            let payload: Value = response
                .json()
                .await
                .map_err(|_| RpcError::Internal("xAI models response was not valid JSON".into()))?;
            let mut models: Vec<crate::services::model_fetch::FetchedModel> = payload["data"]
                .as_array()
                .map(|rows| {
                    rows.iter()
                        .filter_map(|row| {
                            Some(crate::services::model_fetch::FetchedModel {
                                id: row.get("id")?.as_str()?.to_string(),
                                owned_by: row
                                    .get("owned_by")
                                    .and_then(Value::as_str)
                                    .map(str::to_string),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            models.sort_by(|a, b| a.id.cmp(&b.id));
            Ok(serde_json::to_value(models)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_opencode_models" => Ok(serde_json::to_value(
            crate::commands::get_opencode_models()
                .await
                .map_err(RpcError::Internal)?,
        )
        .map_err(|e| RpcError::Internal(e.to_string()))?),
        "fetch_models_for_config" => {
            let base_url = arg::<String>(&args, "baseUrl")?;
            let api_key = arg::<String>(&args, "apiKey")?;
            let is_full_url = optional_arg::<bool>(&args, "isFullUrl")?;
            let models_url = optional_arg::<String>(&args, "modelsUrl")?;
            let custom_user_agent = optional_arg::<String>(&args, "customUserAgent")?;
            let api_format = optional_arg::<String>(&args, "apiFormat")?;
            let request_headers = optional_arg::<std::collections::BTreeMap<String, String>>(
                &args,
                "requestHeaders",
            )?;
            let models = crate::commands::fetch_models_for_config(
                base_url,
                api_key,
                is_full_url,
                models_url,
                custom_user_agent,
                api_format,
                request_headers,
            )
            .await
            .map_err(RpcError::Internal)?;
            Ok(serde_json::to_value(models)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "test_proxy_url" => {
            let url = arg::<String>(&args, "url")?;
            let result = crate::commands::test_proxy_url(url)
                .await
                .map_err(RpcError::Internal)?;
            Ok(serde_json::to_value(result)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "scan_local_proxies" => {
            let proxies = crate::commands::scan_local_proxies().await;
            Ok(serde_json::to_value(proxies)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "read_live_provider_settings" => {
            let app = arg::<String>(&args, "app")?;
            let value = crate::services::ProviderService::read_live_settings(app_type(&app)?)?;
            Ok(value)
        }
        "get_custom_endpoints" => {
            let app = arg::<String>(&args, "app")?;
            let provider_id = arg::<String>(&args, "providerId")?;
            let endpoints = crate::services::ProviderService::get_custom_endpoints(
                &ctx.state,
                app_type(&app)?,
                &provider_id,
            )?;
            Ok(serde_json::to_value(endpoints)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "add_custom_endpoint" => {
            let app = arg::<String>(&args, "app")?;
            let provider_id = arg::<String>(&args, "providerId")?;
            let url = arg::<String>(&args, "url")?;
            crate::services::ProviderService::add_custom_endpoint(
                &ctx.state,
                app_type(&app)?,
                &provider_id,
                url,
            )?;
            Ok(json!(null))
        }
        "remove_custom_endpoint" => {
            let app = arg::<String>(&args, "app")?;
            let provider_id = arg::<String>(&args, "providerId")?;
            let url = arg::<String>(&args, "url")?;
            crate::services::ProviderService::remove_custom_endpoint(
                &ctx.state,
                app_type(&app)?,
                &provider_id,
                url,
            )?;
            Ok(json!(null))
        }
        "update_endpoint_last_used" => {
            let app = arg::<String>(&args, "app")?;
            let provider_id = arg::<String>(&args, "providerId")?;
            let url = arg::<String>(&args, "url")?;
            crate::services::ProviderService::update_endpoint_last_used(
                &ctx.state,
                app_type(&app)?,
                &provider_id,
                url,
            )?;
            Ok(json!(null))
        }
        "test_api_endpoints" => {
            let urls = arg::<Vec<String>>(&args, "urls")?;
            let timeout_secs = optional_arg::<u64>(&args, "timeoutSecs")?;
            let results = crate::services::SpeedtestService::test_endpoints(urls, timeout_secs)
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(results)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_claude_desktop_status" => {
            let proxy_running = ctx.state.proxy_service.is_running().await;
            let status = crate::claude_desktop_config::get_status(
                ctx.state.db.as_ref(),
                proxy_running,
            )
            .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(status)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_claude_desktop_default_routes" => {
            let routes = crate::claude_desktop_config::default_proxy_routes();
            Ok(serde_json::to_value(routes)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "read_omo_local_file" => {
            let data = crate::services::OmoService::read_local_file(
                &crate::services::omo::STANDARD,
            )
            .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(data)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "read_omo_slim_local_file" => {
            let data = crate::services::OmoService::read_local_file(
                &crate::services::omo::SLIM,
            )
            .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(data)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_current_omo_provider_id" => {
            let provider = ctx
                .state
                .db
                .get_current_omo_provider("opencode", "omo")
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(provider.map(|p| p.id).unwrap_or_default()))
        }
        "get_current_omo_slim_provider_id" => {
            let provider = ctx
                .state
                .db
                .get_current_omo_provider("opencode", "omo-slim")
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(provider.map(|p| p.id).unwrap_or_default()))
        }
        "disable_current_omo" | "disable_current_omo_slim" => {
            let category = if command == "disable_current_omo" {
                "omo"
            } else {
                "omo-slim"
            };
            let providers = ctx
                .state
                .db
                .get_all_providers("opencode")
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            for (id, provider) in &providers {
                if provider.category.as_deref() == Some(category) {
                    ctx.state
                        .db
                        .clear_omo_provider_current("opencode", id, category)
                        .map_err(|e| RpcError::Internal(e.to_string()))?;
                }
            }
            let flavor = if command == "disable_current_omo" {
                &crate::services::omo::STANDARD
            } else {
                &crate::services::omo::SLIM
            };
            crate::services::OmoService::delete_config_file(flavor)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(null))
        }
        "get_pi_current_state" => {
            let state = crate::services::pi_state::PiStateService::current(&ctx.state)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(state)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "update_pi_provider_usage_script" => {
            let id = arg::<String>(&args, "id")?;
            let usage_script = arg::<crate::provider::UsageScript>(&args, "usageScript")?;
            let updated = crate::services::ProviderService::update_pi_usage_script(
                &ctx.state,
                &id,
                usage_script,
            )
            .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(updated))
        }
        "get_pi_session_discovery" => {
            let discovery = crate::session_manager::providers::pi::session_discovery();
            Ok(serde_json::to_value(discovery)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "import_opencode_providers_from_live" => {
            let count = crate::services::provider::import_opencode_providers_from_live(&ctx.state)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(count))
        }
        "import_openclaw_providers_from_live" => {
            let count = crate::services::provider::import_openclaw_providers_from_live(&ctx.state)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(count))
        }
        "import_hermes_providers_from_live" => {
            let count = crate::services::provider::import_hermes_providers_from_live(&ctx.state)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(count))
        }
        "get_openclaw_live_provider_ids" => {
            let providers = crate::openclaw_config::get_providers()
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(providers.keys().cloned().collect::<Vec<_>>()))
        }
        "get_hermes_live_provider_ids" => {
            let providers = crate::hermes_config::get_providers()
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(providers.keys().cloned().collect::<Vec<_>>()))
        }
        "get_openclaw_live_provider" => {
            let provider_id = arg::<String>(&args, "providerId")?;
            let provider = crate::openclaw_config::get_provider(&provider_id)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(provider))
        }
        "scan_openclaw_config_health" => {
            let warnings = crate::openclaw_config::scan_openclaw_config_health()
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(warnings)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_openclaw_default_model" => {
            let model = crate::openclaw_config::get_default_model()
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(model)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "set_openclaw_default_model" => {
            let model = arg::<crate::openclaw_config::OpenClawDefaultModel>(&args, "model")?;
            let outcome = crate::openclaw_config::set_default_model(&model)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(outcome)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_openclaw_model_catalog" => {
            let catalog = crate::openclaw_config::get_model_catalog()
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(catalog)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "set_openclaw_model_catalog" => {
            let catalog = arg::<
                std::collections::HashMap<String, crate::openclaw_config::OpenClawModelCatalogEntry>,
            >(&args, "catalog")?;
            let outcome = crate::openclaw_config::set_model_catalog(&catalog)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(outcome)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_openclaw_agents_defaults" => {
            let defaults = crate::openclaw_config::get_agents_defaults()
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(defaults)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "set_openclaw_agents_defaults" => {
            let defaults =
                arg::<crate::openclaw_config::OpenClawAgentsDefaults>(&args, "defaults")?;
            let outcome = crate::openclaw_config::set_agents_defaults(&defaults)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(outcome)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_openclaw_env" => {
            let env = crate::openclaw_config::get_env_config()
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(env)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "set_openclaw_env" => {
            let env = arg::<crate::openclaw_config::OpenClawEnvConfig>(&args, "env")?;
            let outcome = crate::openclaw_config::set_env_config(&env)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(outcome)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_openclaw_tools" => {
            let tools = crate::openclaw_config::get_tools_config()
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(tools)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "set_openclaw_tools" => {
            let tools = arg::<crate::openclaw_config::OpenClawToolsConfig>(&args, "tools")?;
            let outcome = crate::openclaw_config::set_tools_config(&tools)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(outcome)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_hermes_model_config" => {
            let config = crate::hermes_config::get_model_config()
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(config)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "get_hermes_memory" => {
            let kind = arg::<crate::hermes_config::MemoryKind>(&args, "kind")?;
            let content = crate::hermes_config::read_memory(kind)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(content))
        }
        "set_hermes_memory" => {
            let kind = arg::<crate::hermes_config::MemoryKind>(&args, "kind")?;
            let content = arg::<String>(&args, "content")?;
            crate::hermes_config::write_memory(kind, &content)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(null))
        }
        "get_hermes_memory_limits" => {
            let limits = crate::hermes_config::read_memory_limits()
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(limits)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "set_hermes_memory_enabled" => {
            let kind = arg::<crate::hermes_config::MemoryKind>(&args, "kind")?;
            let enabled = arg::<bool>(&args, "enabled")?;
            let outcome = crate::hermes_config::set_memory_enabled(kind, enabled)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(serde_json::to_value(outcome)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "apply_claude_plugin_config" => {
            let official = arg::<bool>(&args, "official")?;
            if official {
                crate::claude_plugin::clear_claude_config()
                    .map_err(|e| RpcError::Internal(e.to_string()))?;
            } else {
                crate::claude_plugin::write_claude_config()
                    .map_err(|e| RpcError::Internal(e.to_string()))?;
            }
            Ok(json!(true))
        }
        "apply_claude_onboarding_skip" => {
            let applied = crate::claude_mcp::set_has_completed_onboarding()
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(applied))
        }
        "clear_claude_onboarding_skip" => {
            let cleared = crate::claude_mcp::clear_has_completed_onboarding()
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!(cleared))
        }
        "is_portable_mode" => {
            let exe = std::env::current_exe()
                .map_err(|e| RpcError::Internal(format!("获取可执行路径失败: {e}")))?;
            Ok(json!(exe
                .parent()
                .map(|dir| dir.join("portable.ini").is_file())
                .unwrap_or(false)))
        }
        "get_tool_versions" => {
            let tools = optional_arg::<Vec<String>>(&args, "tools")?;
            let wsl = optional_arg::<
                std::collections::HashMap<String, crate::commands::WslShellPreferenceInput>,
            >(&args, "wslShellByTool")?;
            let versions = crate::commands::get_tool_versions(tools, wsl)
                .await
                .map_err(RpcError::Internal)?;
            Ok(serde_json::to_value(versions)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "prepare_tool_uninstall" => {
            let tool = arg::<String>(&args, "tool")?;
            let wsl = optional_arg::<
                std::collections::HashMap<String, crate::commands::WslShellPreferenceInput>,
            >(&args, "wslShellByTool")?;
            let candidates = crate::commands::prepare_tool_uninstall(tool, wsl)
                .await
                .map_err(RpcError::Internal)?;
            Ok(serde_json::to_value(candidates)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "run_tool_lifecycle_action" => {
            let tools = arg::<Vec<String>>(&args, "tools")?;
            let action = arg::<String>(&args, "action")?;
            let wsl = optional_arg::<
                std::collections::HashMap<String, crate::commands::WslShellPreferenceInput>,
            >(&args, "wslShellByTool")?;
            let plan = optional_arg::<crate::commands::ToolUninstallPlan>(
                &args,
                "uninstallPlan",
            )?;
            crate::commands::run_tool_lifecycle_action(tools, action, wsl, plan)
                .await
                .map_err(RpcError::Internal)?;
            Ok(json!(null))
        }
        "probe_tool_installations" | "list_tool_installations" => {
            let tools = arg::<Vec<String>>(&args, "tools")?;
            let reports = if command == "probe_tool_installations" {
                crate::commands::probe_tool_installations(tools).await
            } else {
                crate::commands::list_tool_installations(tools).await
            }
            .map_err(RpcError::Internal)?;
            Ok(serde_json::to_value(reports)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "webdav_test_connection" => {
            let mut settings =
                arg::<crate::settings::WebDavSyncSettings>(&args, "settings")?;
            let preserve = args
                .get("preserveEmptyPassword")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            if preserve && settings.password.is_empty() {
                if let Some(existing) = crate::settings::get_webdav_sync_settings() {
                    settings.password = existing.password;
                }
            }
            crate::services::webdav_sync::check_connection(&settings)
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!({ "success": true, "message": "WebDAV connection ok" }))
        }
        "webdav_sync_upload" => {
            let mut settings = crate::settings::get_webdav_sync_settings()
                .ok_or_else(|| RpcError::BadRequest("未配置 WebDAV 同步".into()))?;
            if !settings.enabled {
                return Err(RpcError::BadRequest("WebDAV 同步未启用".into()));
            }
            let db = ctx.state.db.clone();
            let result = crate::services::sync_protocol::run_with_sync_lock(
                crate::services::webdav_sync::upload(&db, &mut settings),
            )
            .await;
            match result {
                Ok(value) => Ok(value),
                Err(error) => {
                    persist_webdav_error(&mut settings, &error);
                    Err(RpcError::Internal(error.to_string()))
                }
            }
        }
        "webdav_sync_download" => {
            let mut settings = crate::settings::get_webdav_sync_settings()
                .ok_or_else(|| RpcError::BadRequest("未配置 WebDAV 同步".into()))?;
            if !settings.enabled {
                return Err(RpcError::BadRequest("WebDAV 同步未启用".into()));
            }
            let db = ctx.state.db.clone();
            let result = crate::services::sync_protocol::run_with_sync_lock(
                crate::services::webdav_sync::download(&db, &mut settings),
            )
            .await;
            match result {
                Ok(value) => Ok(value),
                Err(error) => {
                    persist_webdav_error(&mut settings, &error);
                    Err(RpcError::Internal(error.to_string()))
                }
            }
        }
        "webdav_sync_save_settings" => {
            let incoming =
                arg::<crate::settings::WebDavSyncSettings>(&args, "settings")?;
            let password_touched = args
                .get("passwordTouched")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let existing = crate::settings::get_webdav_sync_settings();
            let mut sync_settings = incoming;
            if !password_touched && sync_settings.password.is_empty() {
                if let Some(existing) = existing.clone() {
                    sync_settings.password = existing.password;
                }
            }
            if let Some(existing) = existing {
                sync_settings.status = existing.status;
            }
            sync_settings.normalize();
            sync_settings
                .validate()
                .map_err(|e| RpcError::BadRequest(e.to_string()))?;
            crate::settings::set_webdav_sync_settings(Some(sync_settings))
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!({ "success": true }))
        }
        "webdav_sync_fetch_remote_info" => {
            let settings = crate::settings::get_webdav_sync_settings()
                .ok_or_else(|| RpcError::BadRequest("未配置 WebDAV 同步".into()))?;
            if !settings.enabled {
                return Err(RpcError::BadRequest("WebDAV 同步未启用".into()));
            }
            let info = crate::services::webdav_sync::fetch_remote_info(&settings)
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(info.unwrap_or(json!({ "empty": true })))
        }
        "s3_test_connection" => {
            let mut settings = arg::<crate::settings::S3SyncSettings>(&args, "settings")?;
            let preserve = args
                .get("preserveEmptyPassword")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            if preserve && settings.secret_access_key.is_empty() {
                if let Some(existing) = crate::settings::get_s3_sync_settings() {
                    settings.secret_access_key = existing.secret_access_key;
                }
            }
            crate::services::s3_sync::check_connection(&settings)
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!({ "success": true, "message": "S3 connection ok" }))
        }
        "s3_sync_upload" => {
            let mut settings = crate::settings::get_s3_sync_settings()
                .ok_or_else(|| RpcError::BadRequest("未配置 S3 同步".into()))?;
            if !settings.enabled {
                return Err(RpcError::BadRequest("S3 同步未启用".into()));
            }
            let db = ctx.state.db.clone();
            let result = crate::services::sync_protocol::run_with_sync_lock(
                crate::services::s3_sync::upload(&db, &mut settings),
            )
            .await;
            match result {
                Ok(value) => Ok(value),
                Err(error) => {
                    persist_s3_error(&mut settings, &error);
                    Err(RpcError::Internal(error.to_string()))
                }
            }
        }
        "s3_sync_download" => {
            let mut settings = crate::settings::get_s3_sync_settings()
                .ok_or_else(|| RpcError::BadRequest("未配置 S3 同步".into()))?;
            if !settings.enabled {
                return Err(RpcError::BadRequest("S3 同步未启用".into()));
            }
            let db = ctx.state.db.clone();
            let result = crate::services::sync_protocol::run_with_sync_lock(
                crate::services::s3_sync::download(&db, &mut settings),
            )
            .await;
            match result {
                Ok(value) => Ok(value),
                Err(error) => {
                    persist_s3_error(&mut settings, &error);
                    Err(RpcError::Internal(error.to_string()))
                }
            }
        }
        "s3_sync_save_settings" => {
            let incoming = arg::<crate::settings::S3SyncSettings>(&args, "settings")?;
            let password_touched = args
                .get("passwordTouched")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let existing = crate::settings::get_s3_sync_settings();
            let mut sync_settings = incoming;
            if !password_touched && sync_settings.secret_access_key.is_empty() {
                if let Some(existing) = existing.clone() {
                    sync_settings.secret_access_key = existing.secret_access_key;
                }
            }
            if let Some(existing) = existing {
                sync_settings.status = existing.status;
            }
            sync_settings.normalize();
            sync_settings
                .validate()
                .map_err(|e| RpcError::BadRequest(e.to_string()))?;
            crate::settings::set_s3_sync_settings(Some(sync_settings))
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!({ "success": true }))
        }
        "s3_sync_fetch_remote_info" => {
            let settings = crate::settings::get_s3_sync_settings()
                .ok_or_else(|| RpcError::BadRequest("未配置 S3 同步".into()))?;
            if !settings.enabled {
                return Err(RpcError::BadRequest("S3 同步未启用".into()));
            }
            let info = crate::services::s3_sync::fetch_remote_info(&settings)
                .await
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(info.unwrap_or(json!({ "empty": true })))
        }
        "set_proxy_stack_member" => {
            let app_str = arg::<String>(&args, "appType")?;
            let provider_id = arg::<String>(&args, "providerId")?;
            let enabled = arg::<bool>(&args, "enabled")?;
            let app = require_mode_app(&app_str)?;
            let result = crate::mode::controller::set_stack_member(
                &ctx.state,
                &app,
                &provider_id,
                enabled,
            )
            .await;
            match result {
                Ok(hint) => {
                    ctx.events.publish(
                        "provider-switched",
                        json!({ "appType": app_str, "providerId": provider_id }),
                    );
                    Ok(json!(hint))
                }
                Err(error) => Err(RpcError::Internal(error.message)),
            }
        }
        "stream_check_provider" => {
            let app_str = arg::<String>(&args, "appType")?;
            let provider_id = arg::<String>(&args, "providerId")?;
            let app_ty = app_type(&app_str)?;
            let providers = ctx
                .state
                .db
                .get_all_providers(app_ty.as_str())
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            let provider = providers.get(&provider_id).ok_or_else(|| {
                RpcError::BadRequest(format!("供应商 {provider_id} 不存在"))
            })?;
            let is_copilot = provider
                .meta
                .as_ref()
                .and_then(|meta| meta.provider_type.as_deref())
                == Some("github_copilot")
                || provider
                    .settings_config
                    .pointer("/env/ANTHROPIC_BASE_URL")
                    .and_then(Value::as_str)
                    .map(|url| url.contains("githubcopilot.com"))
                    .unwrap_or(false);
            let is_full_url = provider
                .meta
                .as_ref()
                .and_then(|meta| meta.is_full_url)
                .unwrap_or(false);
            let base_url_override = if is_copilot && !is_full_url {
                let manager = ctx.copilot.read().await;
                let account_id = provider
                    .meta
                    .as_ref()
                    .and_then(|meta| meta.managed_account_id_for("github_copilot"));
                Some(match account_id.as_deref() {
                    Some(id) => manager.get_api_endpoint(id).await,
                    None => manager.get_default_api_endpoint().await,
                })
            } else {
                None
            };
            let result = crate::services::stream_check::StreamCheckService::check_with_retry(
                &app_ty,
                provider,
                &crate::services::stream_check::StreamCheckConfig::default(),
                base_url_override,
            )
            .await?;
            Ok(serde_json::to_value(result)
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        "import_claude_desktop_providers_from_claude" => {
            let claude_providers = ctx
                .state
                .db
                .get_all_providers(AppType::Claude.as_str())
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            let existing_ids = ctx
                .state
                .db
                .get_provider_ids(AppType::ClaudeDesktop.as_str())
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            let mut imported = 0usize;
            for provider in claude_providers.values() {
                if existing_ids.contains(&provider.id) {
                    continue;
                }
                let mut desktop_provider = provider.clone();
                desktop_provider.in_failover_queue = false;
                let meta = desktop_provider.meta.get_or_insert_with(Default::default);
                if crate::claude_desktop_config::is_compatible_direct_provider(provider)
                    && crate::commands::claude_provider_models_are_claude_safe(provider)
                {
                    meta.claude_desktop_mode = Some(crate::provider::ClaudeDesktopMode::Direct);
                } else if let Some(routes) =
                    crate::commands::suggested_claude_desktop_routes(provider)
                {
                    meta.claude_desktop_mode =
                        Some(crate::provider::ClaudeDesktopMode::Proxy);
                    meta.claude_desktop_model_routes = routes;
                } else {
                    continue;
                }
                ctx.state
                    .db
                    .save_provider(AppType::ClaudeDesktop.as_str(), &desktop_provider)
                    .map_err(|e| RpcError::Internal(e.to_string()))?;
                imported += 1;
            }
            let _ = ctx.state.db.ensure_official_seed_by_id(
                crate::database::CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID,
                AppType::ClaudeDesktop,
            );
            Ok(json!(imported))
        }
        "get_migration_result" => Ok(json!(crate::init_status::take_migration_success())),
        "get_skills_migration_result" => {
            Ok(serde_json::to_value(crate::init_status::take_skills_migration_result())
                .map_err(|e| RpcError::Internal(e.to_string()))?)
        }
        other => Err(RpcError::Unsupported(other.to_string())),
    }
}

fn persist_webdav_error(
    settings: &mut crate::settings::WebDavSyncSettings,
    error: &AppError,
) {
    settings.status.last_error = Some(error.to_string());
    settings.status.last_error_source = Some("manual".to_string());
    let _ = crate::settings::update_webdav_sync_status(settings.status.clone());
}

fn persist_s3_error(settings: &mut crate::settings::S3SyncSettings, error: &AppError) {
    settings.status.last_error = Some(error.to_string());
    settings.status.last_error_source = Some("manual".to_string());
    let _ = crate::settings::update_s3_sync_status(settings.status.clone());
}

const AUTH_COPILOT: &str = "github_copilot";
const AUTH_CODEX: &str = "codex_oauth";
const AUTH_XAI: &str = "xai_oauth";

fn ensure_auth_provider(provider: &str) -> Result<(), RpcError> {
    match provider {
        AUTH_COPILOT | AUTH_CODEX | AUTH_XAI => Ok(()),
        other => Err(RpcError::BadRequest(format!(
            "不支持的认证提供方: {other}"
        ))),
    }
}

fn account_json(
    provider: &str,
    account: crate::proxy::providers::copilot_auth::GitHubAccount,
    default_id: Option<&str>,
) -> Value {
    let is_default = default_id == Some(account.id.as_str());
    json!({
        "id": account.id,
        "provider": provider,
        "login": account.login,
        "avatarUrl": account.avatar_url,
        "authenticatedAt": account.authenticated_at,
        "isDefault": is_default,
        "githubDomain": account.github_domain,
        "reauthRequired": account.reauth_required,
        "requiresReauth": false,
    })
}

fn xai_account_json(
    provider: &str,
    account: crate::proxy::providers::xai_oauth_auth::XaiOAuthAccount,
    default_id: Option<&str>,
) -> Value {
    let is_default = default_id == Some(account.id.as_str());
    json!({
        "id": account.id,
        "provider": provider,
        "login": account.login,
        "avatarUrl": account.avatar_url,
        "authenticatedAt": account.authenticated_at,
        "isDefault": is_default,
        "githubDomain": account.github_domain,
        "reauthRequired": false,
        "requiresReauth": account.requires_reauth,
    })
}

async fn auth_snapshot(
    ctx: &WebCtx,
    provider: &str,
) -> Result<(bool, Option<String>, Option<String>, Vec<Value>), RpcError> {
    match provider {
        AUTH_COPILOT => {
            let manager = ctx.copilot.read().await;
            let status = manager.get_status().await;
            let default_id = status.default_account_id.clone();
            let accounts = status
                .accounts
                .into_iter()
                .map(|account| account_json(provider, account, default_id.as_deref()))
                .collect();
            Ok((
                status.authenticated,
                default_id,
                status.migration_error,
                accounts,
            ))
        }
        AUTH_CODEX => {
            let status = ctx.state.codex_oauth_manager.get_status().await;
            let default_id = status.default_account_id.clone();
            let accounts = status
                .accounts
                .into_iter()
                .map(|account| account_json(provider, account, default_id.as_deref()))
                .collect();
            Ok((status.authenticated, default_id, None, accounts))
        }
        AUTH_XAI => {
            let manager = ctx.xai.read().await;
            let status = manager.get_status().await;
            let default_id = status.default_account_id.clone();
            let accounts = status
                .accounts
                .into_iter()
                .map(|account| xai_account_json(provider, account, default_id.as_deref()))
                .collect();
            Ok((status.authenticated, default_id, None, accounts))
        }
        _ => Err(RpcError::BadRequest("不支持的认证提供方".into())),
    }
}

async fn import_deeplink_unified(
    ctx: &WebCtx,
    request: crate::deeplink::DeepLinkImportRequest,
) -> Result<Value, RpcError> {
    use crate::deeplink::{
        import_mcp_from_deeplink, import_prompt_from_deeplink, import_provider_from_deeplink,
        import_skill_from_deeplink,
    };

    match request.resource.as_str() {
        "provider" => {
            let provider_id = import_provider_from_deeplink(&ctx.state, request)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!({ "type": "provider", "id": provider_id }))
        }
        "prompt" => {
            let prompt_id = import_prompt_from_deeplink(&ctx.state, request)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!({ "type": "prompt", "id": prompt_id }))
        }
        "mcp" => {
            let result = import_mcp_from_deeplink(&ctx.state, request)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!({
                "type": "mcp",
                "importedCount": result.imported_count,
                "importedIds": result.imported_ids,
                "failed": result.failed,
            }))
        }
        "skill" => {
            let skill_key = import_skill_from_deeplink(&ctx.state, request)
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            Ok(json!({ "type": "skill", "key": skill_key }))
        }
        other => Err(RpcError::BadRequest(format!(
            "不支持的深链接资源类型: {other}"
        ))),
    }
}

fn publish_profile_events(ctx: &WebCtx, profile_id: &str, scope: ProfileScope) {
    for app_type in scope.apps() {
        let app_str = app_type.as_str();
        let (_, auto_failover_enabled) = ctx.state.db.get_proxy_flags_sync(app_str);
        let proxy_enabled = crate::mode::current::is_proxy(app_type);
        let provider_id = crate::mode::current::provider_for(
            &ctx.state.db,
            app_type,
            crate::mode::current::Purpose::InUse,
        )
        .ok()
        .flatten()
        .unwrap_or_default();
        ctx.events.publish(
            "provider-switched",
            json!({
                "appType": app_str,
                "proxyEnabled": proxy_enabled,
                "autoFailoverEnabled": auto_failover_enabled,
                "providerId": provider_id,
            }),
        );
    }
    ctx.events.publish(
        "profile-applied",
        json!({ "profileId": profile_id, "scope": scope.as_str() }),
    );
}

async fn save_settings(ctx: &WebCtx, args: Value) -> Result<Value, RpcError> {
    let incoming: AppSettings = arg(&args, "settings")?;
    let existing = get_settings();
    let merged = merge_settings_for_save(incoming, &existing);

    let unify_codex_changed =
        merged.unify_codex_session_history != existing.unify_codex_session_history;
    let unify_codex_enabled = merged.unify_codex_session_history;
    let stack_settings_changed = merged.codex_stack_classic_tools
        != existing.codex_stack_classic_tools
        || merged.codex_stack_subagent_model != existing.codex_stack_subagent_model
        || merged.codex_stack_subagent_reasoning_effort
            != existing.codex_stack_subagent_reasoning_effort
        || merged.codex_stack_auto_review_model != existing.codex_stack_auto_review_model;

    update_settings(merged).map_err(|e| RpcError::Internal(e.to_string()))?;

    if stack_settings_changed {
        if let Err(err) = crate::mode::controller::resync_codex_stack_catalog(&ctx.state).await {
            log::warn!("Codex 聚合设置变更后重写客户端配置失败，回滚设置: {err}");
            let mut rollback = get_settings();
            rollback.codex_stack_classic_tools = existing.codex_stack_classic_tools;
            rollback.codex_stack_subagent_model = existing.codex_stack_subagent_model.clone();
            rollback.codex_stack_subagent_reasoning_effort =
                existing.codex_stack_subagent_reasoning_effort.clone();
            rollback.codex_stack_auto_review_model =
                existing.codex_stack_auto_review_model.clone();
            if let Err(rollback_err) = update_settings(rollback) {
                log::error!("回滚 Codex 聚合设置失败: {rollback_err}");
            }
            return Err(RpcError::Internal(format!(
                "聚合设置未生效（Codex 配置重写失败）: {err}"
            )));
        }
    }

    if unify_codex_changed {
        if let Err(err) =
            crate::services::provider::reapply_current_codex_official_live(&ctx.state)
        {
            log::warn!("统一 Codex 会话历史开关变更后重写 live 配置失败，回滚设置: {err}");
            if let Err(rollback_err) = update_settings(existing.clone()) {
                log::error!("回滚统一会话开关设置失败: {rollback_err}");
            }
            return Err(RpcError::Internal(format!(
                "统一 Codex 会话历史开关未生效（live 配置重写失败）: {err}"
            )));
        }

        if unify_codex_enabled {
            tokio::task::spawn_blocking(|| {
                match crate::codex_history_migration::maybe_migrate_codex_official_history_to_unified_bucket() {
                    Ok(outcome) => {
                        if outcome.skipped_reason.is_none() {
                            log::info!(
                                "✓ Codex official history unify migration completed: jsonl_files={}, state_rows={}",
                                outcome.migrated_jsonl_files,
                                outcome.migrated_state_rows
                            );
                        }
                    }
                    Err(e) => log::warn!("✗ Codex official history unify migration failed: {e}"),
                }
            });
        } else {
            if let Err(err) = crate::settings::clear_codex_official_history_unify_migration() {
                log::warn!("清除统一会话迁移标记失败: {err}");
            }
            if let Err(err) = crate::settings::clear_codex_unify_migrate_existing() {
                log::warn!("清除统一会话迁移意愿失败: {err}");
            }
        }
    }

    ctx.events.publish("settings-updated", json!({}));
    Ok(json!(true))
}

/// 与桌面版 save_settings 相同的密钥保留语义：
/// 前端拿到的密码为空表示“保持现有”，不能把已保存的凭据冲掉。
fn merge_settings_for_save(
    mut incoming: AppSettings,
    existing: &AppSettings,
) -> AppSettings {
    match (&mut incoming.webdav_sync, &existing.webdav_sync) {
        (None, _) => incoming.webdav_sync = existing.webdav_sync.clone(),
        (Some(incoming_sync), Some(existing_sync))
            if incoming_sync.password.is_empty() && !existing_sync.password.is_empty() =>
        {
            incoming_sync.password = existing_sync.password.clone();
        }
        _ => {}
    }
    match (&mut incoming.s3_sync, &existing.s3_sync) {
        (None, _) => incoming.s3_sync = existing.s3_sync.clone(),
        (Some(incoming_sync), Some(existing_sync))
            if incoming_sync.secret_access_key.is_empty()
                && !existing_sync.secret_access_key.is_empty() =>
        {
            incoming_sync.secret_access_key = existing_sync.secret_access_key.clone();
        }
        _ => {}
    }
    incoming.local_migrations = existing.local_migrations.clone();
    incoming.proxy_client_host = existing.proxy_client_host.clone();
    incoming
}
