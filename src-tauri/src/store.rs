//! 配置与日志的持久化：~/.cc-activator/config.json + logs/activator.log

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn default_format() -> String {
    "anthropic".to_string()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderConfig {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub model: String,
    /// "anthropic" | "openai"
    #[serde(default = "default_format")]
    pub format: String,
    /// 定时激活总开关
    #[serde(default)]
    pub enabled: bool,
    /// 每日定时激活时间，"HH:MM"
    #[serde(default)]
    pub times: Vec<String>,
    /// 来源 cc-switch 的 provider id（同步去重用），手动添加为 None
    #[serde(default)]
    pub cc_provider_id: Option<String>,
    #[serde(default)]
    pub cc_app_type: Option<String>,
    /// 额度查询高级配置：ZenMux 用量端点 URL
    #[serde(default)]
    pub usage_url: Option<String>,
    /// 火山方舟用量查询 AccessKey ID（与推理 Key 是两套凭据）
    #[serde(default)]
    pub access_key_id: Option<String>,
    #[serde(default)]
    pub secret_access_key: Option<String>,
    /// 无法靠 base_url 区分时显式指定，目前仅 "zhipu_team"
    #[serde(default)]
    pub plan_type: Option<String>,
    /// 智谱团队版组织 ID / 项目 ID
    #[serde(default)]
    pub team_organization_id: Option<String>,
    #[serde(default)]
    pub team_project_id: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
    /// 供应商 id -> 最近一次定时触发的去重键 "id|日期|HH:MM"
    #[serde(default)]
    pub last_fired: HashMap<String, String>,
    /// 供应商 id -> 最近一次成功激活的时刻（毫秒时间戳）。
    /// 定时激活据此判断上一窗口（+5h）是否仍未过期，见 scheduler::scheduled_delay_ms
    #[serde(default)]
    pub last_activated: HashMap<String, i64>,
    /// 开机自启默认开启的一次性标记。缺失 = 尚未引导（旧配置/全新安装），
    /// 启动时会自动补开自启并写标记；标记存在后即以顶栏开关的注册状态为准。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autostart_defaulted: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogEntry {
    pub ts: i64,
    #[serde(default)]
    pub provider_id: String,
    #[serde(default)]
    pub provider_name: String,
    /// activate | quota | sync | schedule
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub detail: String,
}

pub fn data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".cc-activator")
}

impl Config {
    pub fn load(path: &Path) -> Config {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// 原子写入：先写临时文件再改名，避免写坏配置。
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&tmp, text).map_err(|e| e.to_string())?;
        fs::rename(&tmp, path).map_err(|e| e.to_string())
    }
}

pub fn append_log_file(entry: &LogEntry) {
    let dir = data_dir().join("logs");
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let line = serde_json::json!({
        "ts": entry.ts,
        "provider_id": entry.provider_id,
        "provider_name": entry.provider_name,
        "kind": entry.kind,
        "ok": entry.ok,
        "detail": entry.detail,
    });
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("activator.log"))
    {
        let _ = writeln!(f, "{line}");
    }
}

/// 启动时读回最近 n 条日志（文件按时间追加，读尾部后反转回时间正序）。
pub fn load_log_tail(n: usize) -> Vec<LogEntry> {
    let path = data_dir().join("logs").join("activator.log");
    let Ok(content) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut out: Vec<LogEntry> = content
        .lines()
        .rev()
        .filter_map(|l| serde_json::from_str(l).ok())
        .take(n)
        .collect();
    out.reverse();
    out
}
