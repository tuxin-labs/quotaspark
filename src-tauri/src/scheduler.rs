//! 定时调度：每 30 秒检查一次各供应商的每日触发时间。
//! 触发条件：当前时刻落在 [目标时刻, 目标时刻+10 分钟] 内，且该供应商
//! 当天该时刻尚未触发过（去重键持久化在配置里，应用重启也不会重复发）。
//! 上次成功激活点燃的 5 小时窗口尚未过期时，推迟到过期后再发——窗口内的
//! 请求只会被算进旧窗口（HTTP 200 但不点亮新窗口），详见 scheduled_delay_ms。

use crate::commands;
use crate::engine::WINDOW_MS;
use crate::state::{now_ms, Inner};
use crate::store::ProviderConfig;
use chrono::{Local, NaiveTime, Timelike};
use std::sync::{Arc, Mutex};
use tauri::AppHandle;

/// 触发宽限窗口（秒）：应用晚开一会儿也能补上触发
const GRACE_SECS: i64 = 600;

/// 窗口过期后再多等的安全缓冲（毫秒），盖过本机与供应商服务器的时钟偏差
const BUFFER_MS: i64 = 60 * 1000;

/// 定时激活的推迟毫秒数：上次成功激活点燃的 5 小时窗口若尚未过期，
/// 推迟到过期后再发。推迟上限封顶在触发宽限窗口，防止相邻过近的
/// 时段把激活拖出数小时（正常按 5 小时间隔排的时段最多推迟约 1-2 分钟）。
fn scheduled_delay_ms(last_activated_ms: Option<i64>, now_ms: i64, target_ms: i64) -> i64 {
    let Some(t) = last_activated_ms else {
        return 0;
    };
    let raw = t + WINDOW_MS + BUFFER_MS - now_ms;
    if raw <= 0 {
        return 0;
    }
    raw.min((target_ms + GRACE_SECS * 1000 - now_ms).max(0))
}

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
    let now_millis = now_ms();

    let mut due: Vec<(ProviderConfig, i64)> = Vec::new();
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
                let target_ms = now_millis - diff * 1000;
                let delay = scheduled_delay_ms(
                    config.last_activated.get(&p.id).copied(),
                    now_millis,
                    target_ms,
                );
                due.push((p.clone(), delay));
                break; // 一个供应商一个 tick 只触发一次
            }
        }
        fired = !due.is_empty();
    }
    if fired {
        let st = shared.lock().unwrap();
        let _ = st.config.save(&st.config_path);
    }
    for (p, delay) in due {
        commands::spawn_activation(app.clone(), shared.clone(), p, true, delay);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveTime;

    #[test]
    fn test_parse_time() {
        assert!(NaiveTime::parse_from_str("05:30", "%H:%M").is_ok());
        assert!(NaiveTime::parse_from_str("23:59", "%H:%M").is_ok());
        assert!(NaiveTime::parse_from_str("5:30", "%H:%M").is_ok());
        assert!(NaiveTime::parse_from_str("25:00", "%H:%M").is_err());
        assert!(NaiveTime::parse_from_str("abc", "%H:%M").is_err());
    }

    #[test]
    fn test_scheduled_delay_ms() {
        let now = 1_800_000_000_000i64;
        let target = now;
        // 无激活记录 → 立即发
        assert_eq!(scheduled_delay_ms(None, now, target), 0);
        // 上次激活距今超过 5h+60s → 窗口已过期，立即发
        assert_eq!(
            scheduled_delay_ms(Some(now - WINDOW_MS - 61_000), now, target),
            0
        );
        // 窗口还剩 30 秒过期 → 推迟 30s + 60s 缓冲
        assert_eq!(
            scheduled_delay_ms(Some(now - WINDOW_MS + 30_000), now, target),
            90_000
        );
        // 窗口才点燃 1 小时 → 推迟被宽限窗口（600s）封顶
        assert_eq!(
            scheduled_delay_ms(Some(now - 3_600_000), now, target),
            GRACE_SECS * 1000
        );
        // 推迟永远不会越过宽限窗口
        let d = scheduled_delay_ms(Some(now - 1_000), now, target);
        assert!((0..=GRACE_SECS * 1000).contains(&d));
    }
}
