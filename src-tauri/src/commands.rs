//! Tauri 命令层：前端 invoke 的入口。

use crate::cc_sync::{self, SyncReport};
use crate::engine;
use crate::quota;
use crate::state::{log_and_emit, now_ms, AppState, Inner};
use crate::store::{LogEntry, ProviderConfig};
use chrono::NaiveTime;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tauri_plugin_autostart::ManagerExt;

fn notify(app: &AppHandle) {
    let _ = app.emit("state-changed", ());
}

#[derive(Serialize)]
pub struct ProviderCard {
    #[serde(flatten)]
    pub provider: ProviderConfig,
    pub quota: Option<quota::QuotaResult>,
    /// 最近一次激活结果
    pub last: Option<LogEntry>,
    /// 是否支持自动查额度（前端据此禁用查额度入口，避免点了必报错）
    pub supports_quota: bool,
}

fn new_id() -> String {
    format!("p_{}", chrono::Utc::now().timestamp_micros())
}

fn opt_trim(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

#[tauri::command]
pub fn get_providers(state: tauri::State<AppState>) -> Vec<ProviderCard> {
    let st = state.0.lock().unwrap();
    st.config
        .providers
        .iter()
        .map(|p| {
            // 与 quota::dispatch 的路由一致：已知域名，或显式指定的智谱团队版
            let supports_quota = quota::detect_kind(&p.base_url).is_some()
                || p.plan_type
                    .as_deref()
                    .map(|s| s.eq_ignore_ascii_case("zhipu_team"))
                    .unwrap_or(false);
            ProviderCard {
                quota: st.quota.get(&p.id).cloned(),
                last: st
                    .logs
                    .iter()
                    .rev()
                    .find(|l| l.provider_id == p.id && l.kind == "activate")
                    .cloned(),
                provider: p.clone(),
                supports_quota,
            }
        })
        .collect()
}

#[tauri::command]
pub fn get_logs(state: tauri::State<AppState>) -> Vec<LogEntry> {
    let st = state.0.lock().unwrap();
    st.logs.iter().rev().take(200).cloned().collect()
}

/// 从 cc-switch 数据库导入/更新 Claude 供应商（按 cc provider id 去重）。
#[tauri::command]
pub async fn sync_from_cc(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<SyncReport, String> {
    let (list, skipped) = cc_sync::read_cc_providers()?;
    let shared = state.0.clone();
    let (report, detail) = {
        let mut st = shared.lock().unwrap();
        let mut imported = 0usize;
        let mut updated = 0usize;
        for c in list {
            if let Some(existing) = st
                .config
                .providers
                .iter_mut()
                .find(|p| p.cc_provider_id.as_deref() == Some(c.cc_id.as_str()))
            {
                existing.name = c.name;
                existing.base_url = c.base_url;
                existing.api_key = c.api_key;
                if !c.model.is_empty() {
                    existing.model = c.model;
                }
                // 额度查询凭据：cc-switch 里配了（非空）才覆盖，保留本地手动填的值
                if c.plan_type.is_some() {
                    existing.plan_type = c.plan_type;
                }
                if c.usage_url.is_some() {
                    existing.usage_url = c.usage_url;
                }
                if c.access_key_id.is_some() {
                    existing.access_key_id = c.access_key_id;
                }
                if c.secret_access_key.is_some() {
                    existing.secret_access_key = c.secret_access_key;
                }
                if c.team_organization_id.is_some() {
                    existing.team_organization_id = c.team_organization_id;
                }
                if c.team_project_id.is_some() {
                    existing.team_project_id = c.team_project_id;
                }
                updated += 1;
            } else {
                st.config.providers.push(ProviderConfig {
                    id: new_id(),
                    name: c.name,
                    base_url: c.base_url,
                    api_key: c.api_key,
                    model: c.model,
                    format: "anthropic".into(),
                    enabled: false,
                    times: Vec::new(),
                    cc_provider_id: Some(c.cc_id),
                    cc_app_type: Some("claude".into()),
                    usage_url: c.usage_url,
                    access_key_id: c.access_key_id,
                    secret_access_key: c.secret_access_key,
                    plan_type: c.plan_type,
                    team_organization_id: c.team_organization_id,
                    team_project_id: c.team_project_id,
                });
                imported += 1;
            }
        }
        let report = SyncReport {
            imported,
            updated,
            skipped,
            total: imported + updated + skipped,
        };
        let detail = format!(
            "导入 {imported} 个、更新 {updated} 个、跳过 {skipped} 个（官方登录或本地路由占位配置不含可用 Key）；额度查询凭据已按 cc-switch 配置同步",
        );
        let _ = st.config.save(&st.config_path);
        (report, detail)
    };
    log_and_emit(
        &app,
        &shared,
        LogEntry {
            ts: now_ms(),
            provider_id: String::new(),
            provider_name: "同步 cc-switch".into(),
            kind: "sync".into(),
            ok: true,
            detail,
        },
    );
    Ok(report)
}

#[tauri::command]
pub fn save_provider(
    app: AppHandle,
    state: tauri::State<AppState>,
    mut provider: ProviderConfig,
) -> Result<ProviderConfig, String> {
    if provider.name.trim().is_empty() {
        return Err("名称不能为空".into());
    }
    if provider.base_url.trim().is_empty() {
        return Err("Base URL 不能为空".into());
    }
    if provider.api_key.trim().is_empty() {
        return Err("API Key 不能为空".into());
    }
    if provider.model.trim().is_empty() {
        return Err("激活用模型不能为空".into());
    }
    provider.name = provider.name.trim().to_string();
    provider.base_url = provider.base_url.trim().to_string();
    provider.api_key = provider.api_key.trim().to_string();
    provider.model = provider.model.trim().to_string();
    provider.times = provider
        .times
        .iter()
        .map(|t| t.trim().to_string())
        .collect();
    for t in &provider.times {
        if NaiveTime::parse_from_str(t, "%H:%M").is_err() {
            return Err(format!("时间格式错误：{t}（应为 HH:MM）"));
        }
    }
    provider.times.sort();
    provider.times.dedup();
    provider.usage_url = opt_trim(provider.usage_url);
    provider.access_key_id = opt_trim(provider.access_key_id);
    provider.secret_access_key = opt_trim(provider.secret_access_key);
    provider.team_organization_id = opt_trim(provider.team_organization_id);
    provider.team_project_id = opt_trim(provider.team_project_id);
    provider.plan_type = match provider.plan_type.as_deref().map(str::trim) {
        Some(s) if s.eq_ignore_ascii_case("zhipu_team") => Some("zhipu_team".into()),
        _ => None,
    };

    let shared = state.0.clone();
    if provider.id.is_empty() {
        provider.id = new_id();
        {
            let mut st = shared.lock().unwrap();
            st.config.providers.push(provider.clone());
            st.config.save(&st.config_path)?;
        }
        log_and_emit(
            &app,
            &shared,
            LogEntry {
                ts: now_ms(),
                provider_id: provider.id.clone(),
                provider_name: provider.name.clone(),
                kind: "add".into(),
                ok: true,
                detail: format!("已添加供应商「{}」（{}）", provider.name, provider.base_url),
            },
        );
        return Ok(provider);
    }
    {
        let mut st = shared.lock().unwrap();
        match st.config.providers.iter_mut().find(|p| p.id == provider.id) {
            Some(p) => *p = provider.clone(),
            None => return Err("供应商不存在".into()),
        }
        st.config.save(&st.config_path)?;
    }
    notify(&app);
    Ok(provider)
}

#[tauri::command]
pub fn delete_provider(app: AppHandle, state: tauri::State<AppState>, id: String) -> Result<(), String> {
    let shared = state.0.clone();
    let (name, detail) = {
        let mut st = shared.lock().unwrap();
        let name = st
            .config
            .providers
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.clone())
            .ok_or_else(|| "供应商不存在".to_string())?;
        st.config.providers.retain(|p| p.id != id);
        st.quota.remove(&id);
        st.config.last_fired.remove(&id);
        st.config.save(&st.config_path)?;
        let detail = format!("已删除供应商「{name}」（下次同步会按 cc-switch 现状重新导入）");
        (name, detail)
    };
    log_and_emit(
        &app,
        &shared,
        LogEntry {
            ts: now_ms(),
            provider_id: id,
            provider_name: name,
            kind: "delete".into(),
            ok: true,
            detail,
        },
    );
    Ok(())
}

#[tauri::command]
pub fn set_schedule(
    app: AppHandle,
    state: tauri::State<AppState>,
    id: String,
    enabled: bool,
    times: Vec<String>,
) -> Result<(), String> {
    let times: Vec<String> = times.iter().map(|t| t.trim().to_string()).collect();
    for t in &times {
        if NaiveTime::parse_from_str(t, "%H:%M").is_err() {
            return Err(format!("时间格式错误：{t}（应为 HH:MM）"));
        }
    }
    let mut st = state.0.lock().unwrap();
    let p = st
        .config
        .providers
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| "供应商不存在".to_string())?;
    p.enabled = enabled;
    p.times = times;
    st.config.save(&st.config_path)?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub fn get_autostart(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    let auto = app.autolaunch();
    if enabled {
        auto.enable().map_err(|e| e.to_string())
    } else {
        auto.disable().map_err(|e| e.to_string())
    }
}

/// 立即激活：传 id 激活单个，不传激活全部。
#[tauri::command]
pub async fn activate_now(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    id: Option<String>,
) -> Result<usize, String> {
    let shared = state.0.clone();
    match id {
        Some(id) => {
            let p = {
                let st = shared.lock().unwrap();
                st.config
                    .providers
                    .iter()
                    .find(|p| p.id == id)
                    .cloned()
                    .ok_or_else(|| "供应商不存在".to_string())?
            };
            spawn_activation(app, shared, p);
            Ok(1)
        }
        None => {
            let n = { shared.lock().unwrap().config.providers.len() };
            activate_all_spawn(app, shared);
            Ok(n)
        }
    }
}

/// 供托盘菜单和命令共用的"全部激活"。
pub fn activate_all_spawn(app: AppHandle, shared: Arc<Mutex<Inner>>) {
    let providers: Vec<ProviderConfig> = {
        let st = shared.lock().unwrap();
        st.config.providers.clone()
    };
    for p in providers {
        spawn_activation(app.clone(), shared.clone(), p);
    }
}

pub fn spawn_activation(app: AppHandle, shared: Arc<Mutex<Inner>>, p: ProviderConfig) {
    tauri::async_runtime::spawn(async move {
        let result = engine::activate(&p).await;
        let (ok, detail) = match result {
            Ok(d) => (true, d),
            Err(e) => (false, e),
        };
        log_and_emit(
            &app,
            &shared,
            LogEntry {
                ts: now_ms(),
                provider_id: p.id.clone(),
                provider_name: p.name.clone(),
                kind: "activate".into(),
                ok,
                detail: detail.clone(),
            },
        );
        // 激活成功后顺手刷新额度（如果该供应商支持）
        if ok && quota::detect_kind(&p.base_url).is_some() {
            fetch_and_store_quota(app, shared, p).await;
        }
    });
}

#[tauri::command]
pub async fn query_quota(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let p = {
        let st = state.0.lock().unwrap();
        st.config
            .providers
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or_else(|| "供应商不存在".to_string())?
    };
    fetch_and_store_quota(app, state.0.clone(), p).await;
    Ok(())
}

/// 查询额度并写入状态 + 日志，完成后发事件让前端刷新。
pub async fn fetch_and_store_quota(app: AppHandle, shared: Arc<Mutex<Inner>>, p: ProviderConfig) {
    let q = quota::query_quota(&p).await;
    {
        let mut st = shared.lock().unwrap();
        st.quota.insert(p.id.clone(), q.clone());
    }
    let detail = if q.ok {
        let mut parts: Vec<String> = q
            .tiers
            .iter()
            .map(|t| {
                let mut s = format!("{} 已用 {:.0}%", t.name, t.utilization);
                if let Some(r) = &t.resets_at {
                    s.push_str(&format!("（{r} 重置）"));
                }
                s
            })
            .collect();
        if let Some(b) = &q.balance_text {
            parts.push(b.clone());
        }
        if parts.is_empty() {
            "查询成功".into()
        } else {
            let body = parts.join("；");
            match quota::detect_kind(&p.base_url).map(quota::detect_label) {
                Some(label) => format!("[{label}] {body}"),
                None => body,
            }
        }
    } else {
        q.error.clone().unwrap_or_else(|| "查询失败".into())
    };
    log_and_emit(
        &app,
        &shared,
        LogEntry {
            ts: now_ms(),
            provider_id: p.id.clone(),
            provider_name: p.name.clone(),
            kind: "quota".into(),
            ok: q.ok,
            detail,
        },
    );
}
