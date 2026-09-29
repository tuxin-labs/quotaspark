//! 定时调度：每 30 秒检查一次各供应商的每日触发时间。
//! 触发条件：当前时刻落在 [目标时刻, 目标时刻+10 分钟] 内，且该供应商
//! 当天该时刻尚未触发过（去重键持久化在配置里，应用重启也不会重复发）。

use crate::commands;
use crate::state::Inner;
use crate::store::ProviderConfig;
use chrono::{Local, NaiveTime, Timelike};
use std::sync::{Arc, Mutex};
use tauri::AppHandle;

/// 触发宽限窗口（秒）：应用晚开一会儿也能补上触发
const GRACE_SECS: i64 = 600;

pub fn spawn_scheduler(app: AppHandle, shared: Arc<Mutex<Inner>>) {
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            run_tick(&app, &shared).await;
        }
    });
}

async fn run_tick(app: &AppHandle, shared: &Arc<Mutex<Inner>>) {
    let now = Local::now();
    let today = now.format("%Y-%m-%d").to_string();
    let now_secs = now.hour() as i64 * 3600 + now.minute() as i64 * 60 + now.second() as i64;

    let mut due: Vec<ProviderConfig> = Vec::new();
    let fired;
    {
        let mut st = shared.lock().unwrap();
        // 借一个直接的 &mut Config，让 providers 与 last_fired 的字段借用可分离
        let config = &mut st.config;
        for p in config.providers.iter_mut() {
            if !p.enabled {
                continue;
            }
            for t in p.times.clone() {
                let t = t.trim().to_string();
                let Ok(tt) = NaiveTime::parse_from_str(&t, "%H:%M") else {
                    continue;
                };
                let target = tt.hour() as i64 * 3600 + tt.minute() as i64 * 60;
                let diff = now_secs - target;
                if !(0..=GRACE_SECS).contains(&diff) {
                    continue;
                }
                let key = format!("{}|{}|{}", p.id, today, t);
                if config.last_fired.get(&p.id).map(String::as_str) == Some(key.as_str()) {
                    continue;
                }
                config.last_fired.insert(p.id.clone(), key);
                due.push(p.clone());
                break; // 一个供应商一个 tick 只触发一次
            }
        }
        fired = !due.is_empty();
    }
    if fired {
        let st = shared.lock().unwrap();
        let _ = st.config.save(&st.config_path);
    }
    for p in due {
        commands::spawn_activation(app.clone(), shared.clone(), p);
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveTime;

    #[test]
    fn test_parse_time() {
        assert!(NaiveTime::parse_from_str("05:30", "%H:%M").is_ok());
        assert!(NaiveTime::parse_from_str("23:59", "%H:%M").is_ok());
        assert!(NaiveTime::parse_from_str("5:30", "%H:%M").is_ok());
        assert!(NaiveTime::parse_from_str("25:00", "%H:%M").is_err());
        assert!(NaiveTime::parse_from_str("abc", "%H:%M").is_err());
    }
}
