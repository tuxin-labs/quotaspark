//! 只读读取 cc-switch 数据库（~/.cc-switch/cc-switch.db）里的 Claude 供应商。
//! 端点与鉴权字段参考 cc-switch（MIT）：settings_config.env 下的
//! ANTHROPIC_BASE_URL / ANTHROPIC_AUTH_TOKEN / ANTHROPIC_MODEL。
//! 额度查询凭据（火山 AK/SK、智谱团队组织/项目 ID、ZenMux 用量端点）在
//! meta.usage_script 里，与 cc-switch 的 UsageScript 字段一一对应。

use rusqlite::OpenFlags;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct CcProvider {
    pub cc_id: String,
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub is_current: bool,
    /// 以下来自 meta.usage_script，None/空 = cc-switch 里没配，同步时不清空本地已填值
    /// 智谱团队版标识（codingPlanProvider == "zhipu_team"）
    pub plan_type: Option<String>,
    /// ZenMux 用量端点 URL（usage_script.baseUrl，token_plan 模板才有意义）
    pub usage_url: Option<String>,
    /// 火山方舟 AccessKey ID / Secret（与推理 Key 是两套凭据）
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    /// 智谱团队版组织 / 项目 ID
    pub team_organization_id: Option<String>,
    pub team_project_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncReport {
    pub imported: usize,
    pub updated: usize,
    pub skipped: usize,
    pub total: usize,
}

pub fn cc_db_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".cc-switch")
        .join("cc-switch.db")
}

pub fn read_cc_providers() -> Result<(Vec<CcProvider>, usize), String> {
    let db = cc_db_path();
    if !db.exists() {
        return Err(format!("未找到 cc-switch 数据库：{}", db.display()));
    }
    // 只读打开：不与 cc-switch 争写锁；WAL 模式下并发读是安全的
    let conn = rusqlite::Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("打开数据库失败：{e}"))?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, settings_config, is_current, meta FROM providers \
             WHERE app_type = 'claude' ORDER BY sort_index",
        )
        .map_err(|e| format!("查询失败：{e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, bool>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|e| format!("查询失败：{e}"))?;

    let mut out = Vec::new();
    let mut skipped = 0usize;
    for row in rows {
        let (id, name, cfg, is_current, meta) = row.map_err(|e| format!("读取行失败：{e}"))?;
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&cfg) else {
            skipped += 1;
            continue;
        };
        let env = v.get("env").cloned().unwrap_or_else(|| v.clone());
        let base = str_field(&env, "ANTHROPIC_BASE_URL").unwrap_or_default();
        let key = str_field(&env, "ANTHROPIC_AUTH_TOKEN")
            .or_else(|| str_field(&env, "ANTHROPIC_API_KEY"))
            .unwrap_or_default();
        let model = str_field(&env, "ANTHROPIC_MODEL")
            .or_else(|| str_field(&env, "ANTHROPIC_DEFAULT_SONNET_MODEL"))
            .unwrap_or_default();
        // 官方登录供应商没有 Key；开启本地路由时 cc-switch 写的是占位符
        // PROXY_MANAGED，两种都无法直接激活，跳过
        if base.is_empty() || key.is_empty() || key == "PROXY_MANAGED" {
            skipped += 1;
            continue;
        }
        let usage = parse_usage_config(&meta);
        out.push(CcProvider {
            cc_id: id,
            name,
            base_url: base,
            api_key: key,
            model,
            is_current,
            plan_type: usage.0,
            usage_url: usage.1,
            access_key_id: usage.2,
            secret_access_key: usage.3,
            team_organization_id: usage.4,
            team_project_id: usage.5,
        });
    }
    Ok((out, skipped))
}

/// parse_usage_config 返回的六元组：(plan_type, usage_url, ak, sk, org, proj)
type UsageCredentials = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// 从 meta JSON 里取 usage_script 的额度查询凭据，映射到本工具的
/// ProviderConfig 字段。返回 (plan_type, usage_url, ak, sk, org, proj)。
/// 与 cc-switch 的字段名（camelCase）保持一致：
/// - templateType == "token_plan" 且 codingPlanProvider == "zhipu_team" → plan_type
/// - templateType == "token_plan" 的 baseUrl 是 ZenMux 用量端点
/// - accessKeyId / secretAccessKey / teamOrganizationId / teamProjectId 直取
fn parse_usage_config(meta_json: &str) -> UsageCredentials {
    let Ok(meta) = serde_json::from_str::<serde_json::Value>(meta_json) else {
        return (None, None, None, None, None, None);
    };
    let Some(us) = meta.get("usage_script") else {
        return (None, None, None, None, None, None);
    };
    let template = str_field(us, "templateType");
    let coding_plan = str_field(us, "codingPlanProvider");
    let is_token_plan = template.as_deref() == Some("token_plan");
    let plan_type = if is_token_plan && coding_plan.as_deref() == Some("zhipu_team") {
        Some("zhipu_team".into())
    } else {
        None
    };
    let usage_url = if is_token_plan {
        str_field(us, "baseUrl")
    } else {
        None
    };
    (
        plan_type,
        usage_url,
        str_field(us, "accessKeyId"),
        str_field(us, "secretAccessKey"),
        str_field(us, "teamOrganizationId"),
        str_field(us, "teamProjectId"),
    )
}

fn str_field(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::parse_usage_config;

    #[test]
    fn zhipu_team_full_config_maps_all_fields() {
        let meta = r#"{"usage_script":{
            "enabled":true,"templateType":"token_plan","codingPlanProvider":"zhipu_team",
            "teamOrganizationId":"org-123","teamProjectId":"proj-456",
            "accessKeyId":"AKLT-abc","secretAccessKey":"sk-xyz"
        }}"#;
        let (plan_type, usage_url, ak, sk, org, proj) = parse_usage_config(meta);
        assert_eq!(plan_type.as_deref(), Some("zhipu_team"));
        assert_eq!(usage_url, None);
        assert_eq!(ak.as_deref(), Some("AKLT-abc"));
        assert_eq!(sk.as_deref(), Some("sk-xyz"));
        assert_eq!(org.as_deref(), Some("org-123"));
        assert_eq!(proj.as_deref(), Some("proj-456"));
    }

    #[test]
    fn zenmux_base_url_maps_to_usage_url() {
        let meta = r#"{"usage_script":{
            "templateType":"token_plan","codingPlanProvider":"zenmux",
            "baseUrl":"https://api.zenmux.com/v1/usage"
        }}"#;
        let (plan_type, usage_url, ak, _sk, _org, _proj) = parse_usage_config(meta);
        assert_eq!(plan_type, None);
        assert_eq!(
            usage_url.as_deref(),
            Some("https://api.zenmux.com/v1/usage")
        );
        assert_eq!(ak, None);
    }

    #[test]
    fn volcengine_maps_ak_sk_but_not_plan_type() {
        let meta = r#"{"usage_script":{
            "templateType":"token_plan","codingPlanProvider":"volcengine",
            "accessKeyId":"AKLT-v","secretAccessKey":"sk-v"
        }}"#;
        let (plan_type, usage_url, ak, sk, _org, _proj) = parse_usage_config(meta);
        assert_eq!(plan_type, None);
        assert_eq!(usage_url, None);
        assert_eq!(ak.as_deref(), Some("AKLT-v"));
        assert_eq!(sk.as_deref(), Some("sk-v"));
    }

    #[test]
    fn js_templates_are_not_imported() {
        // general/newapi 是 JS 脚本模板，本工具不执行脚本，凭据一概不搬
        let meta = r#"{"usage_script":{
            "templateType":"general","baseUrl":"https://relay.example.com",
            "apiKey":"sk-usage-only","accessToken":"t","userId":"1"
        }}"#;
        let (plan_type, usage_url, ak, sk, org, proj) = parse_usage_config(meta);
        assert_eq!(plan_type, None);
        assert_eq!(usage_url, None);
        assert_eq!(ak, None);
        assert_eq!(sk, None);
        assert_eq!(org, None);
        assert_eq!(proj, None);
    }

    #[test]
    fn missing_or_invalid_meta_yields_all_none() {
        assert_eq!(
            parse_usage_config(r#"{"providerType":"github_copilot"}"#),
            (None, None, None, None, None, None)
        );
        assert_eq!(
            parse_usage_config("not-json"),
            (None, None, None, None, None, None)
        );
    }
}
