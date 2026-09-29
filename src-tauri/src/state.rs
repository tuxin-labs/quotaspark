//! 全局共享状态与日志/事件辅助。

use crate::quota::QuotaResult;
use crate::store::{Config, LogEntry};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

pub struct Inner {
    pub config: Config,
    pub config_path: PathBuf,
    /// 内存中的日志（时间正序），上限 500 条
    pub logs: Vec<LogEntry>,
    /// 供应商 id -> 最近一次额度查询结果
    pub quota: HashMap<String, QuotaResult>,
}

#[derive(Clone)]
pub struct AppState(pub Arc<Mutex<Inner>>);

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// 写日志（内存 + 文件）并通知前端刷新。
pub fn log_and_emit(app: &AppHandle, shared: &Arc<Mutex<Inner>>, entry: LogEntry) {
    {
        let mut st = shared.lock().unwrap();
        crate::store::append_log_file(&entry);
        st.logs.push(entry);
        if st.logs.len() > 500 {
            let over = st.logs.len() - 500;
            st.logs.drain(0..over);
        }
    }
    let _ = app.emit("state-changed", ());
}
