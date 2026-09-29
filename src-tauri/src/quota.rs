//! 额度 / 余额查询：全部端点与解析逻辑移植自 cc-switch（MIT License）。
//!
//! 按 base_url 自动检测：
//! - Coding Plan（窗口百分比）：Kimi For Coding、智谱 GLM 个人版（bigmodel.cn /
//!   api.z.ai）、智谱团队版（需显式指定 + 组织/项目 ID）、MiniMax（中/英）、
//!   ZenMux（需填用量端点 URL）、OpenCode Go、火山方舟 Agent/Coding Plan（需账号 AK/SK）
//! - 按量余额：DeepSeek、StepFun、SiliconFlow（中/英）、OpenRouter、Novita AI
//!
//! 官方 OAuth 订阅（Claude / ChatGPT / Gemini / Copilot / xAI）依赖 cc-switch 的
//! OAuth token 刷新链路，本工具不搬移。

use crate::engine::{client, truncate};
use crate::store::ProviderConfig;
use serde::{Deserialize, Serialize};
use std::time::Duration;

// ── 类型 ────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuotaTier {
    pub name: String,
    /// 已用百分比 0-100（火山 AFP 可能超 100，展示层负责裁剪）
    pub utilization: f64,
    /// 本地时区重置时间展示串，如 "09-28 15:00"
    pub resets_at: Option<String>,
    /// 金额补充说明，如 "$1.20 / $5.00"
    #[serde(default)]
    pub amount: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct QuotaResult {
    pub ok: bool,
    pub plan: Option<String>,
    pub tiers: Vec<QuotaTier>,
    /// 按量余额类结果的展示串
    #[serde(default)]
    pub balance_text: Option<String>,
    pub error: Option<String>,
    pub ts: i64,
}

// ── 供应商识别 ──────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Kimi,
    Zhipu,
    MiniMaxCn,
    MiniMaxEn,
    ZenMux,
    OpencodeGo,
    Volcengine,
    DeepSeek,
    StepFun,
    SiliconFlowCn,
    SiliconFlowEn,
    OpenRouter,
    Novita,
}

pub fn detect_kind(base_url: &str) -> Option<Kind> {
    let u = base_url.to_lowercase();
    // Coding Plan 优先
    if u.contains("api.kimi.com/coding") {
        Some(Kind::Kimi)
    } else if u.contains("bigmodel.cn") || u.contains("api.z.ai") {
        Some(Kind::Zhipu)
    } else if u.contains("api.minimaxi.com") || u.contains("api.minimax.cn") {
        Some(Kind::MiniMaxCn)
    } else if u.contains("api.minimax.io") {
        Some(Kind::MiniMaxEn)
    } else if u.contains("opencode.ai/zen/go") {
        Some(Kind::OpencodeGo)
    } else if u.contains("volces.com/api/plan") || u.contains("volces.com/api/coding") {
        Some(Kind::Volcengine)
    } else if u.contains("zenmux") {
        Some(Kind::ZenMux)
    }
    // 按量余额
    else if u.contains("api.deepseek.com") {
        Some(Kind::DeepSeek)
    } else if u.contains("api.stepfun.ai") || u.contains("api.stepfun.com") {
        Some(Kind::StepFun)
    } else if u.contains("api.siliconflow.cn") {
        Some(Kind::SiliconFlowCn)
    } else if u.contains("api.siliconflow.com") {
        Some(Kind::SiliconFlowEn)
    } else if u.contains("openrouter.ai") {
        Some(Kind::OpenRouter)
    } else if u.contains("api.novita.ai") {
        Some(Kind::Novita)
    } else {
        None
    }
}

pub fn detect_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Kimi => "Kimi For Coding",
        Kind::Zhipu => "智谱 GLM Coding Plan",
        Kind::MiniMaxCn | Kind::MiniMaxEn => "MiniMax Coding Plan",
        Kind::ZenMux => "ZenMux",
        Kind::OpencodeGo => "OpenCode Go",
        Kind::Volcengine => "火山方舟 Agent/Coding Plan",
        Kind::DeepSeek => "DeepSeek（按量）",
        Kind::StepFun => "StepFun（按量）",
        Kind::SiliconFlowCn | Kind::SiliconFlowEn => "SiliconFlow（按量）",
        Kind::OpenRouter => "OpenRouter（按量）",
        Kind::Novita => "Novita AI（按量）",
    }
}

// ── 入口 ────────────────────────────────────────────────────

pub async fn query_quota(p: &ProviderConfig) -> QuotaResult {
    let result = dispatch(p).await;
    match result {
        Ok(mut q) => {
            q.ts = now_ms();
            q
        }
        Err(e) => QuotaResult {
            ok: false,
            error: Some(e),
            ts: now_ms(),
            ..Default::default()
        },
    }
}

async fn dispatch(p: &ProviderConfig) -> Result<QuotaResult, String> {
    // 智谱团队版：base_url 与个人版完全相同，无法自动区分，必须显式指定
    if p.plan_type
        .as_deref()
        .map(|s| s.eq_ignore_ascii_case("zhipu_team"))
        .unwrap_or(false)
    {
        return zhipu_team(p).await;
    }
    let Some(kind) = detect_kind(&p.base_url) else {
        return Err("未识别的供应商域名，暂不支持自动查额度（激活不受影响）".into());
    };
    match kind {
        Kind::Kimi => kimi(p).await,
        Kind::Zhipu => zhipu(p).await,
        Kind::MiniMaxCn => minimax(p, true).await,
        Kind::MiniMaxEn => minimax(p, false).await,
        Kind::ZenMux => zenmux(p).await,
        Kind::OpencodeGo => opencode_go(p).await,
        Kind::Volcengine => volcengine(p).await,
        Kind::DeepSeek => deepseek(p).await,
        Kind::StepFun => stepfun(p).await,
        Kind::SiliconFlowCn => siliconflow(p, true).await,
        Kind::SiliconFlowEn => siliconflow(p, false).await,
        Kind::OpenRouter => openrouter(p).await,
        Kind::Novita => novita(p).await,
    }
}

// ── 通用辅助 ────────────────────────────────────────────────

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

async fn http_get(
    url: &str,
    headers: &[(&'static str, String)],
) -> Result<(reqwest::StatusCode, String), String> {
    let mut req = client().get(url).timeout(Duration::from_secs(15));
    for (k, v) in headers {
        req = req.header(*k, v.as_str());
    }
    let resp = req.send().await.map_err(|e| format!("请求失败：{e}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    Ok((status, text))
}

fn require_key(p: &ProviderConfig) -> Result<String, String> {
    let k = p.api_key.trim();
    if k.is_empty() {
        return Err("API Key 不能为空".into());
    }
    Ok(k.to_string())
}

fn parse_json(text: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(text).map_err(|e| format!("解析失败：{e}"))
}

fn auth_failed(status: reqwest::StatusCode) -> String {
    format!("Key 无效或已过期（HTTP {status}）")
}

fn num(v: &serde_json::Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn num_field(obj: &serde_json::Value, field: &str) -> Option<f64> {
    obj.get(field).and_then(num)
}

/// 重置时间统一转本地时区展示。兼容：秒/毫秒时间戳（数字或字符串）、ISO 8601 字符串。
fn reset_display(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => {
            let s = s.trim();
            if let Ok(n) = s.parse::<i64>() {
                ts_to_local(n)
            } else {
                iso_to_local(s)
            }
        }
        serde_json::Value::Number(_) => v.as_i64().and_then(ts_to_local),
        _ => None,
    }
}

fn ts_to_local(n: i64) -> Option<String> {
    if n <= 0 {
        return None;
    }
    let ms = if n < 1_000_000_000_000 { n * 1000 } else { n };
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| d.with_timezone(&chrono::Local).format("%m-%d %H:%M").to_string())
}

fn iso_to_local(s: &str) -> Option<String> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&chrono::Local).format("%m-%d %H:%M").to_string())
}

// ── Kimi For Coding ─────────────────────────────────────────
// GET https://api.kimi.com/coding/v1/usages（Bearer）

async fn kimi(p: &ProviderConfig) -> Result<QuotaResult, String> {
    let key = require_key(p)?;
    let (status, text) = http_get(
        "https://api.kimi.com/coding/v1/usages",
        &[("Authorization", format!("Bearer {key}")), ("Accept", "application/json".into())],
    )
    .await?;
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(auth_failed(status));
    }
    if !status.is_success() {
        return Err(format!("HTTP {status}：{}", truncate(&text, 200)));
    }
    let body = parse_json(&text)?;

    let mut tiers = Vec::new();
    if let Some(limits) = body.get("limits").and_then(|v| v.as_array()) {
        for item in limits {
            if let Some(d) = item.get("detail") {
                let limit = d.get("limit").and_then(num).unwrap_or(1.0);
                let remaining = d.get("remaining").and_then(num).unwrap_or(0.0);
                let util = if limit > 0.0 {
                    ((limit - remaining).max(0.0) / limit) * 100.0
                } else {
                    0.0
                };
                tiers.push(QuotaTier {
                    name: "五小时窗口".into(),
                    utilization: util,
                    resets_at: d.get("resetTime").and_then(reset_display),
                    amount: None,
                });
            }
        }
    }
    if let Some(u) = body.get("usage") {
        let limit = u.get("limit").and_then(num).unwrap_or(1.0);
        let remaining = u.get("remaining").and_then(num).unwrap_or(0.0);
        let util = if limit > 0.0 {
            ((limit - remaining).max(0.0) / limit) * 100.0
        } else {
            0.0
        };
        tiers.push(QuotaTier {
            name: "本周额度".into(),
            utilization: util,
            resets_at: u.get("resetTime").and_then(reset_display),
            amount: None,
        });
    }
    if tiers.is_empty() {
        return Err("响应中没有额度数据".into());
    }
    Ok(QuotaResult { ok: true, plan: None, tiers, balance_text: None, error: None, ts: 0 })
}

// ── 智谱 GLM（个人版 / 团队版共用解析）──────────────────────
// GET {open.bigmodel.cn | api.z.ai}/api/monitor/usage/quota/limit
// 注意：Authorization 直接放 Key，不带 Bearer 前缀。unit=3 是五小时窗口，unit=6 是周窗口。

fn zhipu_tiers_from_body(
    body: &serde_json::Value,
) -> Result<(Vec<QuotaTier>, Option<String>), String> {
    if body.get("success").and_then(|v| v.as_bool()) == Some(false) {
        return Err(format!(
            "接口错误：{}",
            body.get("msg").and_then(|v| v.as_str()).unwrap_or("未知")
        ));
    }
    let data = body.get("data").ok_or_else(|| "响应缺少 data 字段".to_string())?;
    let plan = data.get("level").and_then(|v| v.as_str()).map(|s| s.to_string());

    let mut five: Option<(f64, Option<String>)> = None;
    let mut weekly: Option<(f64, Option<String>)> = None;
    let mut other: Vec<(f64, Option<String>)> = Vec::new();
    if let Some(limits) = data.get("limits").and_then(|v| v.as_array()) {
        for item in limits {
            let t = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if !t.eq_ignore_ascii_case("TOKENS_LIMIT") && !t.eq_ignore_ascii_case("CREDIT_LIMIT") {
                continue;
            }
            let util = item.get("percentage").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let entry = (util, item.get("nextResetTime").and_then(reset_display));
            match item.get("unit").and_then(|v| v.as_i64()) {
                Some(3) => {
                    if five.is_none() {
                        five = Some(entry);
                    }
                }
                Some(6) => {
                    if weekly.is_none() {
                        weekly = Some(entry);
                    }
                }
                _ => other.push(entry),
            }
        }
    }
    // unit 缺失时的兜底：第一条归五小时，第二条归周（智谱最多两条）
    let mut others = other.into_iter();
    if five.is_none() {
        five = others.next();
    }
    if weekly.is_none() {
        weekly = others.next();
    }

    let mut tiers = Vec::new();
    if let Some((util, reset)) = five {
        tiers.push(QuotaTier { name: "五小时窗口".into(), utilization: util, resets_at: reset, amount: None });
    }
    if let Some((util, reset)) = weekly {
        tiers.push(QuotaTier { name: "本周额度".into(), utilization: util, resets_at: reset, amount: None });
    }
    if tiers.is_empty() {
        return Err("响应中没有额度数据".into());
    }
    Ok((tiers, plan))
}

async fn zhipu(p: &ProviderConfig) -> Result<QuotaResult, String> {
    let key = require_key(p)?;
    let host = if p.base_url.to_lowercase().contains("bigmodel.cn") {
        "https://open.bigmodel.cn"
    } else {
        "https://api.z.ai"
    };
    let url = format!("{host}/api/monitor/usage/quota/limit");
    let (status, text) = http_get(
        &url,
        &[("Authorization", key), ("Accept-Language", "en-US,en".into())],
    )
    .await?;
    if !status.is_success() {
        return Err(format!("HTTP {status}：{}", truncate(&text, 200)));
    }
    let body = parse_json(&text)?;
    let (tiers, plan) = zhipu_tiers_from_body(&body)?;
    Ok(QuotaResult { ok: true, plan, tiers, balance_text: None, error: None, ts: 0 })
}

/// 智谱团队版：同一 quota 路径加 ?type=2，额外携带
/// bigmodel-organization / bigmodel-project 头（三者缺一不可）。
async fn zhipu_team(p: &ProviderConfig) -> Result<QuotaResult, String> {
    let key = require_key(p)?;
    let org = p.team_organization_id.as_deref().unwrap_or("").trim();
    let proj = p.team_project_id.as_deref().unwrap_or("").trim();
    if org.is_empty() || proj.is_empty() {
        return Err("智谱团队版需要在编辑表单的高级配置里填写组织 ID 与项目 ID".into());
    }
    let (status, text) = http_get(
        "https://open.bigmodel.cn/api/monitor/usage/quota/limit?type=2",
        &[
            ("Authorization", key),
            ("bigmodel-organization", org.to_string()),
            ("bigmodel-project", proj.to_string()),
            ("Accept-Language", "en-US,en".into()),
        ],
    )
    .await?;
    if !status.is_success() {
        return Err(format!("HTTP {status}：{}", truncate(&text, 200)));
    }
    let body = parse_json(&text)?;
    let (tiers, plan) = zhipu_tiers_from_body(&body)?;
    Ok(QuotaResult { ok: true, plan, tiers, balance_text: None, error: None, ts: 0 })
}

// ── MiniMax ─────────────────────────────────────────────────
// GET https://{api.minimaxi.com | api.minimax.io}/v1/api/openplatform/coding_plan/remains
// 新接口直接给"剩余百分比"，反转为已用百分比；只取 model_name == "general" 的条目。

async fn minimax(p: &ProviderConfig, is_cn: bool) -> Result<QuotaResult, String> {
    let key = require_key(p)?;
    let domain = if is_cn { "api.minimaxi.com" } else { "api.minimax.io" };
    let url = format!("https://{domain}/v1/api/openplatform/coding_plan/remains");
    let (status, text) =
        http_get(&url, &[("Authorization", format!("Bearer {key}"))]).await?;
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(auth_failed(status));
    }
    if !status.is_success() {
        return Err(format!("HTTP {status}：{}", truncate(&text, 200)));
    }
    let body = parse_json(&text)?;
    if let Some(br) = body.get("base_resp") {
        let code = br.get("status_code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            return Err(format!(
                "接口错误（code {code}）：{}",
                br.get("status_msg").and_then(|v| v.as_str()).unwrap_or("未知")
            ));
        }
    }
    let item = body
        .get("model_remains")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter().find(|i| {
                i.get("model_name").and_then(|v| v.as_str()) == Some("general")
            })
        })
        .ok_or_else(|| "响应中没有 general 模型的额度数据".to_string())?;

    let mut tiers = Vec::new();
    if let Some(remain) = item
        .get("current_interval_remaining_percent")
        .and_then(|v| v.as_f64())
    {
        tiers.push(QuotaTier {
            name: "五小时窗口".into(),
            utilization: 100.0 - remain,
            resets_at: item.get("end_time").and_then(reset_display),
            amount: None,
        });
    }
    // 仅当 status=1 时有周限额；status=3 等表示该套餐无周限额
    if item.get("current_weekly_status").and_then(|v| v.as_i64()) == Some(1) {
        if let Some(remain) = item
            .get("current_weekly_remaining_percent")
            .and_then(|v| v.as_f64())
        {
            tiers.push(QuotaTier {
                name: "本周额度".into(),
                utilization: 100.0 - remain,
                resets_at: item.get("weekly_end_time").and_then(reset_display),
                amount: None,
            });
        }
    }
    if tiers.is_empty() {
        return Err("响应中没有额度数据".into());
    }
    Ok(QuotaResult { ok: true, plan: None, tiers, balance_text: None, error: None, ts: 0 })
}

// ── ZenMux ──────────────────────────────────────────────────
// GET {用量端点 URL，用户在高级配置里填}（Bearer）
// data.quota_5_hour / quota_7_day：usage_percentage 为 0-1 小数，附 USD 金额。

async fn zenmux(p: &ProviderConfig) -> Result<QuotaResult, String> {
    let key = require_key(p)?;
    let url = p
        .usage_url
        .as_deref()
        .unwrap_or("")
        .trim()
        .trim_end_matches('/')
        .to_string();
    if url.is_empty() {
        return Err("ZenMux 需要在编辑表单的高级配置里填写用量端点 URL".into());
    }
    let (status, text) = http_get(
        &url,
        &[("Authorization", format!("Bearer {key}")), ("Accept", "application/json".into())],
    )
    .await?;
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(auth_failed(status));
    }
    if !status.is_success() {
        return Err(format!("HTTP {status}：{}", truncate(&text, 200)));
    }
    let body = parse_json(&text)?;
    if body.get("success").and_then(|v| v.as_bool()) != Some(true) {
        return Err(format!(
            "接口错误：{}",
            body.get("message").and_then(|v| v.as_str()).unwrap_or("未知")
        ));
    }
    let data = body.get("data").ok_or_else(|| "响应缺少 data 字段".to_string())?;

    let mut tiers = Vec::new();
    for (field, name) in [("quota_5_hour", "五小时窗口"), ("quota_7_day", "本周额度")] {
        let Some(q) = data.get(field) else { continue };
        let util = q.get("usage_percentage").and_then(num).unwrap_or(0.0) * 100.0;
        let used = q.get("used_value_usd").and_then(num);
        let max = q.get("max_value_usd").and_then(num);
        let amount = match (used, max) {
            (Some(u), Some(m)) => Some(format!("${u:.2} / ${m:.2}")),
            (Some(u), None) => Some(format!("已用 ${u:.2}")),
            _ => None,
        };
        tiers.push(QuotaTier {
            name: name.into(),
            utilization: util,
            resets_at: q.get("resets_at").and_then(|v| v.as_str()).and_then(iso_to_local),
            amount,
        });
    }
    if tiers.is_empty() {
        return Err("响应中没有额度数据".into());
    }
    let plan = data
        .get("plan")
        .and_then(|pl| pl.get("tier"))
        .and_then(|v| v.as_str())
        .map(|s| format!("ZenMux {s}"));
    Ok(QuotaResult { ok: true, plan, tiers, balance_text: None, error: None, ts: 0 })
}

// ── OpenCode Go ─────────────────────────────────────────────
// GET https://opencode.ai/zen/go/v1/usage（Bearer）
// usage.{rolling|weekly|monthly}：{status, percent(0-100 已用), resetsAt}

async fn opencode_go(p: &ProviderConfig) -> Result<QuotaResult, String> {
    let key = require_key(p)?;
    let (status, text) = http_get(
        "https://opencode.ai/zen/go/v1/usage",
        &[("Authorization", format!("Bearer {key}")), ("Accept", "application/json".into())],
    )
    .await?;
    if status == reqwest::StatusCode::FORBIDDEN {
        return Err("Key 有效但该账号没有 OpenCode Go 订阅（HTTP 403）".into());
    }
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(auth_failed(status));
    }
    if !status.is_success() {
        return Err(format!("HTTP {status}：{}", truncate(&text, 200)));
    }
    let body = parse_json(&text)?;
    let usage = body.get("usage").ok_or_else(|| "响应缺少 usage 字段".to_string())?;

    let mut tiers = Vec::new();
    for (field, name) in
        [("rolling", "五小时窗口"), ("weekly", "本周额度"), ("monthly", "本月额度")]
    {
        let Some(w) = usage.get(field) else { continue };
        let Some(pct) = w.get("percent").and_then(num) else { continue };
        // percent 为 0 时上游的 resetsAt 是占位值，丢弃不展示
        let resets_at = if pct > 0.0 {
            w.get("resetsAt").and_then(reset_display)
        } else {
            None
        };
        tiers.push(QuotaTier { name: name.into(), utilization: pct, resets_at, amount: None });
    }
    if tiers.is_empty() {
        return Err("响应形态不认识（未文档化端点可能已变更）".into());
    }
    Ok(QuotaResult {
        ok: true,
        plan: Some("OpenCode Go".into()),
        tiers,
        balance_text: None,
        error: None,
        ts: 0,
    })
}

// ── 火山方舟 Agent Plan / Coding Plan ───────────────────────
//
// 控制面 OpenAPI（open.volcengineapi.com），强制火山签名 V4（AK/SK）——
// 推理 Bearer Key 会被网关 400 拒绝。算法是 AWS SigV4 的火山变体（移植自
// cc-switch，对照官方 volc-openapi-demos）：
//   1. canonical headers 与 SignedHeaders 用固定顺序 host;x-date;x-content-sha256;content-type
//      （不按字母序）；
//   2. algorithm 串 `HMAC-SHA256`（无 AWS4 前缀）、credential scope 结尾 `request`
//      （非 aws4_request）、签名密钥 kDate=HMAC(SK, date)（SK 不加 AWS4 前缀）。
// 自动探测：先调 GetAFPUsage（Agent Plan），未订阅再调 GetCodingPlanUsage（Coding Plan）。

const VOLC_OPENAPI_HOST: &str = "open.volcengineapi.com";
const VOLC_API_VERSION: &str = "2024-01-01";
const VOLC_DEFAULT_REGION: &str = "cn-beijing";
const VOLC_SERVICE: &str = "ark";
const VOLC_CONTENT_TYPE: &str = "application/json; charset=utf-8";
const VOLC_SIGNED_HEADERS: &str = "host;x-date;x-content-sha256;content-type";
const VOLC_AKSK_HINT: &str =
    "请检查 AccessKey ID / Secret 是否正确、账号是否有方舟用量查询（OpenAPI）权限。";

fn volc_hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    type HmacSha256 = Hmac<sha2::Sha256>;
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn volc_sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(data))
}

/// RFC3986 unreserved 之外全部按 %XX 编码
fn volc_uri_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

/// 从数据面 base_url 提取 Region（ark.cn-beijing.volces.com → cn-beijing）
fn volc_region(base_url: &str) -> String {
    let host = base_url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(base_url)
        .split('/')
        .next()
        .unwrap_or("");
    host.split('.')
        .find(|p| p.starts_with("cn-") || p.starts_with("ap-"))
        .map(|p| p.to_string())
        .unwrap_or_else(|| VOLC_DEFAULT_REGION.to_string())
}

fn volc_canonical_query(action: &str, region: &str) -> String {
    let mut pairs = [
        ("Action", action),
        ("Region", region),
        ("Version", VOLC_API_VERSION),
    ];
    pairs.sort_by(|a, b| a.0.cmp(b.0));
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", volc_uri_encode(k), volc_uri_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// 生成 (Authorization, X-Date, X-Content-Sha256)，三者都要塞进请求头。
fn volc_sign(
    access_key_id: &str,
    secret_access_key: &str,
    region: &str,
    canonical_query: &str,
    body: &[u8],
    now: chrono::DateTime<chrono::Utc>,
) -> (String, String, String) {
    let x_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let short_date = now.format("%Y%m%d").to_string();
    let x_content_sha256 = volc_sha256_hex(body);

    // 火山特有：canonical headers 固定顺序，不按字母序
    let canonical_headers = format!(
        "host:{VOLC_OPENAPI_HOST}\nx-date:{x_date}\nx-content-sha256:{x_content_sha256}\ncontent-type:{VOLC_CONTENT_TYPE}\n"
    );
    let canonical_request = format!(
        "POST\n/\n{canonical_query}\n{canonical_headers}\n{VOLC_SIGNED_HEADERS}\n{x_content_sha256}"
    );

    let credential_scope = format!("{short_date}/{region}/{VOLC_SERVICE}/request");
    let string_to_sign = format!(
        "HMAC-SHA256\n{x_date}\n{credential_scope}\n{}",
        volc_sha256_hex(canonical_request.as_bytes())
    );

    let k_date = volc_hmac(secret_access_key.as_bytes(), short_date.as_bytes());
    let k_region = volc_hmac(&k_date, region.as_bytes());
    let k_service = volc_hmac(&k_region, VOLC_SERVICE.as_bytes());
    let k_signing = volc_hmac(&k_service, b"request");
    let signature: String = volc_hmac(&k_signing, string_to_sign.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();

    let authorization = format!(
        "HMAC-SHA256 Credential={access_key_id}/{credential_scope}, SignedHeaders={VOLC_SIGNED_HEADERS}, Signature={signature}"
    );
    (authorization, x_date, x_content_sha256)
}

fn volc_is_auth_error_code(code: &str) -> bool {
    let c = code.to_lowercase();
    c.contains("auth")
        || c.contains("signature")
        || c.contains("accessdenied")
        || c.contains("denied")
        || c.contains("unauthorized")
        || c.contains("forbidden")
        || c.contains("credential")
        || c.contains("token")
}

fn volc_response_error(body: &serde_json::Value) -> Option<(String, String)> {
    let err = body
        .get("ResponseMetadata")
        .and_then(|m| m.get("Error"))
        .or_else(|| body.get("Error"))?;
    let code = err.get("Code").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let msg = err.get("Message").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if code.is_empty() && msg.is_empty() {
        None
    } else {
        Some((code, msg))
    }
}

/// 单次 OpenAPI 调用的归类结果：鉴权失败（两个 plan 共用 AK/SK，命中即停）
/// 与其他失败分开，便于探测逻辑决定是否继续试另一个 plan。
enum VolcCall {
    Body(serde_json::Value),
    Auth(String),
    Other(String),
}

async fn volc_openapi_call(
    region: &str,
    access_key_id: &str,
    secret_access_key: &str,
    action: &str,
) -> VolcCall {
    let canonical_query = volc_canonical_query(action, region);
    let url = format!("https://{VOLC_OPENAPI_HOST}/?{canonical_query}");
    let body: &[u8] = b"";
    let (authorization, x_date, x_content_sha256) = volc_sign(
        access_key_id,
        secret_access_key,
        region,
        &canonical_query,
        body,
        chrono::Utc::now(),
    );

    let resp = client()
        .post(&url)
        .header("X-Date", x_date)
        .header("X-Content-Sha256", x_content_sha256)
        .header("Content-Type", VOLC_CONTENT_TYPE)
        .header("Authorization", authorization)
        .body(body.to_vec())
        .timeout(Duration::from_secs(15))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return VolcCall::Other(format!("网络错误：{e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return VolcCall::Auth(format!("鉴权失败（HTTP {status}）。{VOLC_AKSK_HINT}"));
    }
    let raw = match resp.text().await {
        Ok(t) => t,
        Err(e) => return VolcCall::Other(format!("读取响应失败：{e}")),
    };
    if !status.is_success() {
        // 火山网关对签名/凭据类错误常返 4xx 并携带 ResponseMetadata.Error 信封
        if let Ok(body) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some((code, msg)) = volc_response_error(&body) {
                if volc_is_auth_error_code(&code) {
                    return VolcCall::Auth(format!(
                        "鉴权失败（HTTP {status}，{code}）：{msg}。{VOLC_AKSK_HINT}"
                    ));
                }
                return VolcCall::Other(format!("接口错误（HTTP {status}，{code}）：{msg}"));
            }
        }
        return VolcCall::Other(format!("接口错误（HTTP {status}）：{}", truncate(&raw, 200)));
    }
    let body = match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(v) => v,
        Err(_) => return VolcCall::Other("响应不是合法 JSON".into()),
    };
    if let Some((code, msg)) = volc_response_error(&body) {
        if volc_is_auth_error_code(&code) {
            return VolcCall::Auth(format!("鉴权失败（{code}）：{msg}。{VOLC_AKSK_HINT}"));
        }
        return VolcCall::Other(format!("接口错误（{code}）：{msg}"));
    }
    VolcCall::Body(body)
}

/// GetAFPUsage：展示 5h / 周 / 月三个窗口；Quota<=0 视为未订阅该窗口。
fn parse_afp_tiers(result: &serde_json::Value) -> Vec<QuotaTier> {
    let mut tiers = Vec::new();
    for (key, name) in [
        ("AFPFiveHour", "五小时窗口"),
        ("AFPWeekly", "本周额度"),
        ("AFPMonthly", "本月额度"),
    ] {
        let Some(win) = result.get(key) else { continue };
        let quota = win.get("Quota").and_then(num).unwrap_or(0.0);
        if quota <= 0.0 {
            continue;
        }
        let used = win.get("Used").and_then(num).unwrap_or(0.0);
        tiers.push(QuotaTier {
            name: name.into(),
            utilization: used / quota * 100.0,
            resets_at: win.get("ResetTime").and_then(reset_display),
            amount: Some(format!("{used:.2} / {quota:.2}")),
        });
    }
    tiers
}

/// GetCodingPlanUsage 的窗口标签归一（真实字段 Level：session/weekly/monthly）
fn volc_window(label: &str) -> Option<&'static str> {
    match label.to_lowercase().as_str() {
        "session" | "5h" | "fivehour" | "five_hour" | "rolling_5h" => Some("五小时窗口"),
        "weekly" | "week" | "7d" => Some("本周额度"),
        "monthly" | "month" => Some("本月额度"),
        _ => None,
    }
}

/// GetCodingPlanUsage：宽松匹配数组与字段名，只给百分比，重置时间是秒级。
fn parse_coding_plan_tiers(result: &serde_json::Value) -> Vec<QuotaTier> {
    let mut tiers = Vec::new();
    let arr = result
        .get("QuotaUsage")
        .and_then(|v| v.as_array())
        .or_else(|| result.get("Usages").and_then(|v| v.as_array()))
        .or_else(|| result.get("Details").and_then(|v| v.as_array()));
    let Some(arr) = arr else { return tiers };

    for item in arr {
        let label = item
            .get("Level")
            .and_then(|v| v.as_str())
            .or_else(|| item.get("Type").and_then(|v| v.as_str()))
            .or_else(|| item.get("Period").and_then(|v| v.as_str()))
            .or_else(|| item.get("Label").and_then(|v| v.as_str()))
            .or_else(|| item.get("Window").and_then(|v| v.as_str()))
            .unwrap_or("");
        let Some(name) = volc_window(label) else { continue };
        let utilization = item
            .get("Percent")
            .and_then(num)
            .or_else(|| item.get("UsedPercent").and_then(num))
            .or_else(|| item.get("UsagePercent").and_then(num))
            .unwrap_or(0.0);
        tiers.push(QuotaTier {
            name: name.into(),
            utilization,
            resets_at: item
                .get("ResetTime")
                .or_else(|| item.get("ResetTimestamp"))
                .and_then(reset_display),
            amount: None,
        });
    }
    tiers
}

async fn volcengine(p: &ProviderConfig) -> Result<QuotaResult, String> {
    let ak = p.access_key_id.as_deref().unwrap_or("").trim();
    let sk = p.secret_access_key.as_deref().unwrap_or("").trim();
    if ak.is_empty() || sk.is_empty() {
        return Err(
            "火山方舟用量查询需要账号的 AccessKey ID + Secret（与推理 API Key 是两套凭据），请在编辑表单的高级配置里填写"
                .into(),
        );
    }
    let region = volc_region(&p.base_url);
    let mut soft_errors: Vec<String> = Vec::new();
    let mut empty_responses: Vec<String> = Vec::new();

    match volc_openapi_call(&region, ak, sk, "GetAFPUsage").await {
        VolcCall::Auth(detail) => return Err(detail),
        VolcCall::Other(detail) => soft_errors.push(format!("GetAFPUsage：{detail}")),
        VolcCall::Body(body) => {
            let result = body.get("Result").unwrap_or(&body);
            let tiers = parse_afp_tiers(result);
            if !tiers.is_empty() {
                let plan = result
                    .get("PlanType")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| format!("Agent Plan {s}"));
                return Ok(QuotaResult { ok: true, plan, tiers, balance_text: None, error: None, ts: 0 });
            }
            empty_responses.push(format!(
                "GetAFPUsage={}",
                truncate(&body.to_string(), 400)
            ));
        }
    }

    match volc_openapi_call(&region, ak, sk, "GetCodingPlanUsage").await {
        VolcCall::Auth(detail) => return Err(detail),
        VolcCall::Other(detail) => soft_errors.push(format!("GetCodingPlanUsage：{detail}")),
        VolcCall::Body(body) => {
            let result = body.get("Result").unwrap_or(&body);
            let tiers = parse_coding_plan_tiers(result);
            if !tiers.is_empty() {
                return Ok(QuotaResult {
                    ok: true,
                    plan: Some("Coding Plan".into()),
                    tiers,
                    balance_text: None,
                    error: None,
                    ts: 0,
                });
            }
            empty_responses.push(format!(
                "GetCodingPlanUsage={}",
                truncate(&body.to_string(), 400)
            ));
        }
    }

    if !soft_errors.is_empty() {
        Err(soft_errors.join("；"))
    } else if !empty_responses.is_empty() {
        Err(format!(
            "签名已通过但没有可解析的额度数据（可能未订阅）。原始响应：{}",
            empty_responses.join(" || ")
        ))
    } else {
        Err("该凭据下没有找到有效的 Agent Plan 或 Coding Plan 订阅".into())
    }
}

// ── 按量余额 ────────────────────────────────────────────────

async fn deepseek(p: &ProviderConfig) -> Result<QuotaResult, String> {
    let key = require_key(p)?;
    let (status, text) = http_get(
        "https://api.deepseek.com/user/balance",
        &[("Authorization", format!("Bearer {key}")), ("Accept", "application/json".into())],
    )
    .await?;
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(auth_failed(status));
    }
    if !status.is_success() {
        return Err(format!("HTTP {status}：{}", truncate(&text, 200)));
    }
    let body = parse_json(&text)?;
    let is_available = body.get("is_available").and_then(|v| v.as_bool()).unwrap_or(true);
    let mut parts = Vec::new();
    if let Some(infos) = body.get("balance_infos").and_then(|v| v.as_array()) {
        for info in infos {
            let currency = info.get("currency").and_then(|v| v.as_str()).unwrap_or("CNY");
            if let Some(total) = info.get("total_balance").and_then(num) {
                parts.push(format!("{currency} {total:.2}"));
            }
        }
    }
    if parts.is_empty() {
        return Err("响应中没有余额数据".into());
    }
    let mut t = format!("DeepSeek 余额：{}", parts.join("；"));
    if !is_available {
        t.push_str("（余额不足）");
    }
    Ok(QuotaResult { ok: true, plan: None, tiers: vec![], balance_text: Some(t), error: None, ts: 0 })
}

async fn stepfun(p: &ProviderConfig) -> Result<QuotaResult, String> {
    let key = require_key(p)?;
    let (status, text) = http_get(
        "https://api.stepfun.com/v1/accounts",
        &[("Authorization", format!("Bearer {key}")), ("Accept", "application/json".into())],
    )
    .await?;
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(auth_failed(status));
    }
    if !status.is_success() {
        return Err(format!("HTTP {status}：{}", truncate(&text, 200)));
    }
    let body = parse_json(&text)?;
    let balance = num_field(&body, "balance").unwrap_or(0.0);
    Ok(QuotaResult {
        ok: true,
        plan: None,
        tiers: vec![],
        balance_text: Some(format!("StepFun 余额：{balance:.2} CNY")),
        error: None,
        ts: 0,
    })
}

async fn siliconflow(p: &ProviderConfig, is_cn: bool) -> Result<QuotaResult, String> {
    let key = require_key(p)?;
    let domain = if is_cn { "api.siliconflow.cn" } else { "api.siliconflow.com" };
    let (status, text) = http_get(
        &format!("https://{domain}/v1/user/info"),
        &[("Authorization", format!("Bearer {key}")), ("Accept", "application/json".into())],
    )
    .await?;
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(auth_failed(status));
    }
    if !status.is_success() {
        return Err(format!("HTTP {status}：{}", truncate(&text, 200)));
    }
    let body = parse_json(&text)?;
    let data = body.get("data").ok_or_else(|| "响应缺少 data 字段".to_string())?;
    let total = num_field(data, "totalBalance").unwrap_or(0.0);
    let unit = if is_cn { "CNY" } else { "USD" };
    let label = if is_cn { "SiliconFlow" } else { "SiliconFlow (EN)" };
    Ok(QuotaResult {
        ok: true,
        plan: None,
        tiers: vec![],
        balance_text: Some(format!("{label} 余额：{total:.2} {unit}")),
        error: None,
        ts: 0,
    })
}

async fn openrouter(p: &ProviderConfig) -> Result<QuotaResult, String> {
    let key = require_key(p)?;
    let (status, text) = http_get(
        "https://openrouter.ai/api/v1/credits",
        &[("Authorization", format!("Bearer {key}")), ("Accept", "application/json".into())],
    )
    .await?;
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(auth_failed(status));
    }
    if !status.is_success() {
        return Err(format!("HTTP {status}：{}", truncate(&text, 200)));
    }
    let body = parse_json(&text)?;
    let data = body.get("data").unwrap_or(&body);
    let total_credits = num_field(data, "total_credits").unwrap_or(0.0);
    let total_usage = num_field(data, "total_usage").unwrap_or(0.0);
    let remaining = total_credits - total_usage;
    Ok(QuotaResult {
        ok: true,
        plan: None,
        tiers: vec![],
        balance_text: Some(format!(
            "OpenRouter 余额：{remaining:.2} USD（充值 {total_credits:.2}，已用 {total_usage:.2}）"
        )),
        error: None,
        ts: 0,
    })
}

async fn novita(p: &ProviderConfig) -> Result<QuotaResult, String> {
    let key = require_key(p)?;
    let (status, text) = http_get(
        "https://api.novita.ai/v3/user/balance",
        &[("Authorization", format!("Bearer {key}")), ("Accept", "application/json".into())],
    )
    .await?;
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(auth_failed(status));
    }
    if !status.is_success() {
        return Err(format!("HTTP {status}：{}", truncate(&text, 200)));
    }
    let body = parse_json(&text)?;
    // 金额单位为 0.0001 USD，除以 10000 转为 USD
    let available = num_field(&body, "availableBalance").unwrap_or(0.0) / 10000.0;
    Ok(QuotaResult {
        ok: true,
        plan: None,
        tiers: vec![],
        balance_text: Some(format!("Novita AI 余额：{available:.2} USD")),
        error: None,
        ts: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_kind() {
        use Kind::*;
        assert_eq!(detect_kind("https://open.bigmodel.cn/api/anthropic"), Some(Zhipu));
        assert_eq!(detect_kind("https://api.z.ai/api/anthropic"), Some(Zhipu));
        assert_eq!(detect_kind("https://api.kimi.com/coding/"), Some(Kimi));
        assert_eq!(detect_kind("https://api.minimax.cn/anthropic"), Some(MiniMaxCn));
        assert_eq!(detect_kind("https://api.minimax.io/anthropic"), Some(MiniMaxEn));
        assert_eq!(detect_kind("https://opencode.ai/zen/go"), Some(OpencodeGo));
        assert_eq!(
            detect_kind("https://ark.cn-beijing.volces.com/api/plan/v3"),
            Some(Volcengine)
        );
        assert_eq!(detect_kind("https://api.deepseek.com/anthropic"), Some(DeepSeek));
        assert_eq!(detect_kind("https://api.siliconflow.cn/v1"), Some(SiliconFlowCn));
        assert_eq!(detect_kind("https://openrouter.ai/api/v1"), Some(OpenRouter));
        assert_eq!(detect_kind("https://api.novita.ai/v3"), Some(Novita));
        assert_eq!(detect_kind("https://api.moonshot.cn/anthropic"), None);
    }

    #[test]
    fn test_zhipu_tiers() {
        let body: serde_json::Value = serde_json::from_str(
            r#"{"success":true,"data":{"level":"GLM Coding Pro","limits":[
                {"type":"TOKENS_LIMIT","unit":3,"percentage":12.5,"nextResetTime":1759000000000},
                {"type":"TOKENS_LIMIT","unit":6,"percentage":45.0,"nextResetTime":1759500000000}
            ]}}"#,
        )
        .unwrap();
        let (tiers, plan) = zhipu_tiers_from_body(&body).unwrap();
        assert_eq!(plan.as_deref(), Some("GLM Coding Pro"));
        assert_eq!(tiers[0].name, "五小时窗口");
        assert!((tiers[0].utilization - 12.5).abs() < 1e-9);
        assert_eq!(tiers[1].name, "本周额度");
        assert!(tiers[0].resets_at.is_some());
    }

    #[test]
    fn test_reset_display() {
        // 秒级时间戳
        let v = serde_json::json!(1759000000i64);
        assert!(reset_display(&v).is_some());
        // ISO 字符串
        let v = serde_json::json!("2026-09-28T15:00:00Z");
        assert!(reset_display(&v).is_some());
        // 0 / 负数视为无
        assert!(reset_display(&serde_json::json!(0)).is_none());
        assert!(reset_display(&serde_json::json!(-1)).is_none());
        // 非法
        assert!(reset_display(&serde_json::json!("abc")).is_none());
    }

    #[test]
    fn test_volc_uri_encode() {
        assert_eq!(volc_uri_encode("a b+c/d"), "a%20b%2Bc%2Fd");
        assert_eq!(volc_uri_encode("Action"), "Action");
        assert_eq!(volc_uri_encode("a~b-c_d.e"), "a~b-c_d.e");
    }

    #[test]
    fn test_volc_region() {
        assert_eq!(volc_region("https://ark.cn-beijing.volces.com/api/plan/v3"), "cn-beijing");
        assert_eq!(volc_region("https://ark.ap-southeast.volces.com/api/coding"), "ap-southeast");
        assert_eq!(volc_region("https://example.com"), VOLC_DEFAULT_REGION);
    }

    #[test]
    fn test_volc_coding_window() {
        assert_eq!(volc_window("session"), Some("五小时窗口"));
        assert_eq!(volc_window("weekly"), Some("本周额度"));
        assert_eq!(volc_window("monthly"), Some("本月额度"));
        assert_eq!(volc_window("daily"), None);
    }
}
