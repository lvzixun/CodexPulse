use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    pub mode: String,
    pub interval_seconds: u64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            mode: "auto".into(),
            interval_seconds: 300,
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if !["auto", "manual"].contains(&self.mode.as_str()) {
            return Err("刷新方式无效".into());
        }
        if !(30..=86400).contains(&self.interval_seconds) {
            return Err("刷新间隔应为 30 秒至 24 小时".into());
        }
        Ok(())
    }
}

/// One logical flight survives invalidation until its completion: a new generation
/// cannot overlap an old transport that is still returning / timing out.
pub struct Schedule {
    pub generation: u64,
    pub flight: Option<u64>,
    pub requested: bool,
    pub next: i64,
    started: bool,
}
impl Schedule {
    pub fn new() -> Self {
        Self {
            generation: 0,
            flight: None,
            requested: false,
            next: 0,
            started: false,
        }
    }
    pub fn request(&mut self) {
        if self.flight.is_none() {
            self.requested = true;
        }
    }
    pub fn invalidate(&mut self, next: i64) {
        self.generation = self.generation.wrapping_add(1);
        self.requested = false;
        self.next = next;
    }
    pub fn due(&self, config: &Config, now: i64, blocked_until: i64) -> bool {
        self.flight.is_none()
            && now >= blocked_until
            && (self.requested || (config.mode == "auto" && now >= self.next))
    }
    /// Only account automation is visibility-aware. The first dispatched flight
    /// may run before a window exists; invalidation/rebuilding never renews that
    /// allowance. Explicit requests retain the existing manual-mode semantics.
    pub fn due_for_account(
        &self,
        config: &Config,
        now: i64,
        blocked_until: i64,
        visible: bool,
    ) -> bool {
        (visible || !self.started || self.requested) && self.due(config, now, blocked_until)
    }
    pub fn begin(&mut self) -> u64 {
        self.requested = false;
        self.started = true;
        self.flight = Some(self.generation);
        self.generation
    }
    pub fn finish(&mut self, generation: u64, next: i64) -> bool {
        if self.flight != Some(generation) {
            return false;
        }
        self.flight = None;
        if self.generation != generation {
            return false;
        }
        self.next = next;
        true
    }
    pub fn status(&self, config: &Config, now: i64, blocked_until: i64) -> &'static str {
        if self.flight.is_some() {
            "refreshing"
        } else if now < blocked_until {
            "backoff"
        } else if self.requested {
            "queued"
        } else if config.mode == "manual" {
            "manual"
        } else {
            "idle"
        }
    }
}

pub fn retry_after(value: Option<&str>, now: i64) -> i64 {
    value
        .and_then(|v| {
            v.parse::<i64>().ok().filter(|n| *n >= 0).or_else(|| {
                chrono::DateTime::parse_from_rfc2822(v)
                    .ok()
                    .map(|d| (d.timestamp() - now).max(0))
            })
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manual_has_no_startup_timer_and_clicks_coalesce_without_overlap() {
        let config = Config {
            mode: "manual".into(),
            ..Config::default()
        };
        let mut s = Schedule::new();
        assert!(!s.due(&config, 100, 0));
        s.request();
        s.request();
        assert!(s.due(&config, 100, 0));
        let old = s.begin();
        s.request();
        assert!(!s.due(&config, 1000, 0));
        assert!(s.finish(old, 101));
        assert!(!s.due(&config, 1000, 0));
    }
    #[test]
    fn settings_invalidate_old_flight_and_groups_keep_independent_intervals() {
        let config = Config::default();
        let mut a = Schedule::new();
        let mut b = Schedule::new();
        let old = a.begin();
        a.invalidate(400);
        assert!(!a.due(&config, 500, 0));
        assert!(!a.finish(old, 900));
        assert!(a.due(&config, 500, 0));
        let current = a.begin();
        assert!(a.finish(current, 1000));
        b.next = 700;
        assert!(b.due(&config, 800, 0));
        assert!(!a.due(&config, 800, 0));
        b.request();
        assert!(!b.due(&config, 800, 900));
        assert_eq!(retry_after(Some("1200"), 0), 1200);
        assert_eq!(
            retry_after(Some("Wed, 21 Oct 2015 07:28:00 GMT"), 1445412470),
            10
        );
    }
    #[test]
    fn account_startup_hidden_pause_and_cache_expiry_keep_news_independent() {
        let config = Config {
            interval_seconds: 30,
            ..Config::default()
        };
        let mut account = Schedule::new();
        let mut news = Schedule::new();
        // Startup has no window, but both automatic groups dispatch once.
        assert!(account.due_for_account(&config, 100, 0, false));
        assert!(news.due(&config, 100, 0));
        let account_flight = account.begin();
        let news_flight = news.begin();
        assert!(account.finish(account_flight, 130));
        assert!(news.finish(news_flight, 130));
        let mut news_refreshes = 1;
        // More than two account intervals hidden: news retains its timer.
        for now in 101..=200 {
            assert!(!account.due_for_account(&config, now, 0, false));
            if news.due(&config, now, 0) {
                let flight = news.begin();
                assert!(news.finish(flight, now + 30));
                news_refreshes += 1;
            }
        }
        assert_eq!(news_refreshes, 4);
        assert!(account.due_for_account(&config, 200, 0, true));
        let flight = account.begin();
        // Repeated shows and manual clicks while in flight do not add requests.
        account.request();
        for visible in [false, true, false, true] {
            assert!(!account.due_for_account(&config, 201, 0, visible));
        }
        // Hiding lets the accepted flight finish and retain its cache deadline.
        assert!(account.finish(flight, 231));
        for now in 202..231 {
            assert!(!account.due_for_account(&config, now, 0, true));
        }
        assert!(account.due_for_account(&config, 231, 0, true));
        assert!(!account.due_for_account(&config, 231, 0, false));
    }

    #[test]
    fn account_manual_mode_does_not_gain_a_startup_or_visibility_timer() {
        let config = Config {
            mode: "manual".into(),
            ..Config::default()
        };
        let mut account = Schedule::new();
        for visible in [false, true] {
            assert!(!account.due_for_account(&config, 100, 0, visible));
        }
        account.request();
        account.request();
        assert!(account.due_for_account(&config, 100, 0, false));
        let flight = account.begin();
        assert!(account.finish(flight, 400));
        for visible in [true, false, true] {
            assert!(!account.due_for_account(&config, 1000, 0, visible));
        }
    }

    #[test]
    fn account_show_hide_and_invalidation_preserve_backoff_and_startup_consumption() {
        let config = Config::default();
        let mut account = Schedule::new();
        let flight = account.begin();
        // A failed first flight still consumes the startup allowance.
        assert!(account.finish(flight, 400));
        for now in [200, 400, 500, 599] {
            for visible in [false, true] {
                assert!(!account.due_for_account(&config, now, 600, visible));
            }
        }
        // Credential/config invalidation does not grant another hidden flight.
        account.invalidate(0);
        assert!(!account.due_for_account(&config, 600, 600, false));
        assert!(account.due_for_account(&config, 600, 600, true));
        let old = account.begin();
        account.invalidate(0);
        assert!(!account.due_for_account(&config, 700, 600, true));
        assert!(!account.finish(old, 900));
        assert!(!account.due_for_account(&config, 700, 600, false));
        assert!(account.due_for_account(&config, 700, 600, true));
        account.request();
        assert!(!account.due_for_account(&config, 700, 800, false));
        assert!(account.due_for_account(&config, 800, 800, false));
    }
}
