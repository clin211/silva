//! 按登录邮箱的连续失败锁定，防止在线猜测密码。
//!
//! 状态保存在进程内存中，与 API 的按 IP 限流同为单实例方案：多副本部署时各副本独立计数，
//! 进程重启后清空。键为规范化邮箱，不论账户是否存在都同样计数和锁定，不能借此枚举账户。
//! 所有方法显式接收当前时间，便于在单元测试中注入。

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

/// 默认连续失败阈值。
pub const DEFAULT_THRESHOLD: u32 = 5;
/// 默认锁定时长。
pub const DEFAULT_LOCK_DURATION: Duration = Duration::from_secs(900);
/// 统计连续失败的固定窗口，自窗口内第一次尝试起算。
pub const FAILURE_WINDOW: Duration = Duration::from_secs(900);
/// 最多跟踪的登录键数量，防止大量随机邮箱耗尽内存。
pub const DEFAULT_MAX_KEYS: usize = 10_000;

/// 一次登录尝试的准入结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// 允许校验密码；本次尝试已预先计入失败次数，调用方须在结束时调用
    /// [`LoginLockout::succeed`] 或 [`LoginLockout::abandon`]，凭据失败则无需再调用。
    Allowed,
    /// 处于锁定期，不得校验密码。
    Locked {
        /// 剩余锁定时长。
        retry_after: Duration,
        /// 是否为本轮锁定后首次被拒；仅首次需要写审计，避免锁定期间的请求刷爆审计表。
        first_rejection: bool,
    },
}

pub struct LoginLockout {
    threshold: u32,
    window: Duration,
    lock_duration: Duration,
    max_keys: usize,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    entries: HashMap<String, Entry>,
    last_sweep: Option<Instant>,
}

struct Entry {
    window_start: Instant,
    attempts: u32,
    locked_until: Option<Instant>,
    lock_reported: bool,
}

impl Entry {
    fn fresh(now: Instant) -> Self {
        Self {
            window_start: now,
            attempts: 0,
            locked_until: None,
            lock_reported: false,
        }
    }

    /// 锁定已到期或窗口已结束的记录不再影响后续登录，可以清理或重置。
    fn expired(&self, now: Instant, window: Duration) -> bool {
        match self.locked_until {
            Some(until) => now >= until,
            None => now.duration_since(self.window_start) >= window,
        }
    }
}

impl Default for LoginLockout {
    fn default() -> Self {
        Self::new(DEFAULT_THRESHOLD, DEFAULT_LOCK_DURATION)
    }
}

impl LoginLockout {
    pub fn new(threshold: u32, lock_duration: Duration) -> Self {
        Self::with_limits(threshold, FAILURE_WINDOW, lock_duration, DEFAULT_MAX_KEYS)
    }

    pub fn with_limits(
        threshold: u32,
        window: Duration,
        lock_duration: Duration,
        max_keys: usize,
    ) -> Self {
        Self {
            threshold: threshold.max(1),
            window,
            lock_duration,
            max_keys: max_keys.max(1),
            state: Mutex::default(),
        }
    }

    /// 判断是否允许本次尝试，并在允许时预先计入一次失败。
    ///
    /// 预先计数使并发请求无法在第一次失败落账前越过阈值：达到阈值的那次尝试仍会校验密码，
    /// 同时开始锁定；它若成功，[`Self::succeed`] 会解除锁定。
    pub fn admit(&self, key: &str, now: Instant) -> Admission {
        // 状态在任何中断点都保持一致，锁中毒时继续使用内部数据，避免登录永久不可用。
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let window = self.window;
        // 按需清理过期记录：表满或距上次清理超过一个窗口时才全表扫描，避免每个请求 O(n)。
        let sweep_due = state
            .last_sweep
            .is_none_or(|last| now.duration_since(last) >= window);
        if sweep_due || state.entries.len() >= self.max_keys {
            state.entries.retain(|_, entry| !entry.expired(now, window));
            state.last_sweep = Some(now);
        }
        if state.entries.len() >= self.max_keys && !state.entries.contains_key(key) {
            // 优先淘汰窗口最早的未锁定记录；全部处于锁定时拒绝新键，等待最早的锁定到期。
            let oldest = state
                .entries
                .iter()
                .filter(|(_, entry)| entry.locked_until.is_none())
                .min_by_key(|(_, entry)| entry.window_start)
                .map(|(key, _)| key.clone());
            match oldest {
                Some(oldest) => {
                    state.entries.remove(&oldest);
                }
                None => {
                    let until = state
                        .entries
                        .values()
                        .filter_map(|entry| entry.locked_until)
                        .min()
                        .unwrap_or(now + self.lock_duration);
                    return Admission::Locked {
                        retry_after: until.saturating_duration_since(now),
                        first_rejection: false,
                    };
                }
            }
        }
        let entry = state
            .entries
            .entry(key.to_owned())
            .or_insert_with(|| Entry::fresh(now));
        if entry.expired(now, window) {
            *entry = Entry::fresh(now);
        }
        if let Some(until) = entry.locked_until {
            let first_rejection = !entry.lock_reported;
            entry.lock_reported = true;
            return Admission::Locked {
                retry_after: until.saturating_duration_since(now),
                first_rejection,
            };
        }
        entry.attempts += 1;
        if entry.attempts >= self.threshold {
            entry.locked_until = Some(now + self.lock_duration);
        }
        Admission::Allowed
    }

    /// 登录成功：清除该键的失败计数与锁定。
    pub fn succeed(&self, key: &str) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.entries.remove(key);
    }

    /// 尝试因凭据以外的原因（如数据库故障）结束：撤回 [`Self::admit`] 预先计入的失败。
    pub fn abandon(&self, key: &str) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(entry) = state.entries.get_mut(key) {
            entry.attempts = entry.attempts.saturating_sub(1);
            if entry.attempts < self.threshold {
                entry.locked_until = None;
                entry.lock_reported = false;
            }
        }
    }
}

/// 将剩余时长换算为 `Retry-After` 秒数：向上取整且至少为 1。
pub fn retry_after_seconds(retry_after: Duration) -> u64 {
    let seconds = retry_after.as_secs() + u64::from(retry_after.subsec_nanos() > 0);
    seconds.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    fn lockout() -> LoginLockout {
        LoginLockout::with_limits(3, 60 * SECOND, 30 * SECOND, 100)
    }

    fn fail(lockout: &LoginLockout, key: &str, now: Instant, times: u32) {
        for _ in 0..times {
            assert_eq!(lockout.admit(key, now), Admission::Allowed);
        }
    }

    #[test]
    fn locks_after_threshold_and_rejects_even_valid_password() {
        let lockout = lockout();
        let start = Instant::now();
        fail(&lockout, "a@example.test", start, 3);
        // 锁定期内的请求不再进入密码校验，首次被拒标记需要写审计。
        assert_eq!(
            lockout.admit("a@example.test", start + 10 * SECOND),
            Admission::Locked {
                retry_after: 20 * SECOND,
                first_rejection: true
            }
        );
        assert_eq!(
            lockout.admit("a@example.test", start + 11 * SECOND),
            Admission::Locked {
                retry_after: 19 * SECOND,
                first_rejection: false
            }
        );
        // 其他键不受影响。
        assert_eq!(
            lockout.admit("b@example.test", start + 11 * SECOND),
            Admission::Allowed
        );
    }

    #[test]
    fn below_threshold_is_not_locked() {
        let lockout = lockout();
        let start = Instant::now();
        fail(&lockout, "a@example.test", start, 2);
        assert_eq!(lockout.admit("a@example.test", start), Admission::Allowed);
    }

    #[test]
    fn failures_outside_window_are_forgotten() {
        let lockout = lockout();
        let start = Instant::now();
        fail(&lockout, "a@example.test", start, 2);
        // 窗口结束后重新计数，因此还需再失败 3 次才锁定。
        let later = start + 60 * SECOND;
        fail(&lockout, "a@example.test", later, 3);
        assert!(matches!(
            lockout.admit("a@example.test", later),
            Admission::Locked { .. }
        ));
    }

    #[test]
    fn lock_expiry_resets_counter() {
        let lockout = lockout();
        let start = Instant::now();
        fail(&lockout, "a@example.test", start, 3);
        let expired = start + 30 * SECOND;
        fail(&lockout, "a@example.test", expired, 2);
        assert_eq!(lockout.admit("a@example.test", expired), Admission::Allowed);
        // 新一轮锁定重新标记首次被拒。
        assert!(matches!(
            lockout.admit("a@example.test", expired),
            Admission::Locked {
                first_rejection: true,
                ..
            }
        ));
    }

    #[test]
    fn success_clears_failures_and_lock() {
        let lockout = lockout();
        let start = Instant::now();
        fail(&lockout, "a@example.test", start, 2);
        lockout.succeed("a@example.test");
        fail(&lockout, "a@example.test", start, 2);
        assert_eq!(lockout.admit("a@example.test", start), Admission::Allowed);
        // 达到阈值的那次尝试密码正确时同样解除锁定。
        lockout.succeed("a@example.test");
        assert_eq!(lockout.admit("a@example.test", start), Admission::Allowed);
    }

    #[test]
    fn abandon_refunds_non_credential_failures() {
        let lockout = lockout();
        let start = Instant::now();
        fail(&lockout, "a@example.test", start, 2);
        assert_eq!(lockout.admit("a@example.test", start), Admission::Allowed);
        lockout.abandon("a@example.test");
        assert_eq!(lockout.admit("a@example.test", start), Admission::Allowed);
        assert!(matches!(
            lockout.admit("a@example.test", start),
            Admission::Locked { .. }
        ));
    }

    #[test]
    fn unknown_accounts_are_locked_the_same_way() {
        // 状态机只看键，不感知账户是否存在；调用方对任何邮箱都使用同一流程。
        let lockout = lockout();
        let start = Instant::now();
        fail(&lockout, "missing@example.test", start, 3);
        assert!(matches!(
            lockout.admit("missing@example.test", start),
            Admission::Locked { .. }
        ));
    }

    #[test]
    fn capacity_is_bounded() {
        let lockout = LoginLockout::with_limits(2, 60 * SECOND, 30 * SECOND, 2);
        let start = Instant::now();
        fail(&lockout, "old@example.test", start, 1);
        fail(&lockout, "locked@example.test", start, 2);
        // 表满时淘汰最早的未锁定记录，锁定记录保留。
        fail(&lockout, "new@example.test", start + SECOND, 1);
        assert_eq!(lockout.state.lock().unwrap().entries.len(), 2);
        assert!(matches!(
            lockout.admit("locked@example.test", start + SECOND),
            Admission::Locked { .. }
        ));
        // 全部记录都处于锁定时拒绝新键，直到最早的锁定到期。
        fail(&lockout, "new@example.test", start + SECOND, 1);
        assert_eq!(
            lockout.admit("other@example.test", start + 2 * SECOND),
            Admission::Locked {
                retry_after: 28 * SECOND,
                first_rejection: false
            }
        );
        assert_eq!(lockout.state.lock().unwrap().entries.len(), 2);
        // 锁定到期后清理腾出空间。
        assert_eq!(
            lockout.admit("other@example.test", start + 31 * SECOND),
            Admission::Allowed
        );
    }

    #[test]
    fn survives_poisoned_lock() {
        let lockout = std::sync::Arc::new(lockout());
        let poisoner = lockout.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.state.lock().unwrap();
            panic!("poison the lockout lock");
        })
        .join();
        assert!(lockout.state.is_poisoned());
        assert_eq!(
            lockout.admit("a@example.test", Instant::now()),
            Admission::Allowed
        );
    }

    #[test]
    fn retry_after_rounds_up_to_at_least_one_second() {
        assert_eq!(retry_after_seconds(Duration::ZERO), 1);
        assert_eq!(retry_after_seconds(Duration::from_millis(1)), 1);
        assert_eq!(retry_after_seconds(Duration::from_millis(1001)), 2);
        assert_eq!(retry_after_seconds(30 * SECOND), 30);
    }
}
