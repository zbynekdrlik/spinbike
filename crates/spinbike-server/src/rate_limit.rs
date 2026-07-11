//! Shared in-memory sliding-window rate limiter.
//!
//! ONE generic limiter (`SlidingWindowLimiter<K>`) backs both the door route
//! (per-user `i64` keys) and the login-link route (per-email `String` keys),
//! replacing the two hand-rolled copies that had already drifted (#166). Each
//! call slides a global window plus a per-key window and evicts keys that have
//! gone quiet — closing the door limiter's latent per-key-map growth leak that
//! the login variant already guarded against.
//!
//! Single-instance server, so a per-process struct is enough — no Redis. The
//! two routes each hold one behind an `Arc<Mutex<_>>` (via the thin
//! `door::RateLimiter` / `auth::LoginLinkRateLimiter` typed wrappers) so
//! concurrent integration tests get their own throttle windows.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::time::{Duration, Instant};

/// Reason tags returned on rejection. Logged by the routes; the HTTP layer
/// flattens them to a single response per endpoint. Shared by every config —
/// only the per-key cap reason (below) is caller-specific.
const REASON_GLOBAL_CAP: &str = "global_cap";
const REASON_TOO_FAST: &str = "too_fast";

/// Static configuration for one limiter instance.
#[derive(Clone, Copy)]
pub struct RateLimitConfig {
    /// Decision window for a key's recorded hits. Hits STRICTLY older than this
    /// (`age > per_key_window`, so a hit aged exactly at the boundary is KEPT)
    /// no longer count toward `per_key_min_gap` / `per_key_max`.
    pub per_key_window: Duration,
    /// Minimum spacing between two accepted hits for the same key. A hit within
    /// this gap of the key's most-recent hit is rejected as `too_fast`. `None`
    /// disables the min-gap check.
    pub per_key_min_gap: Option<Duration>,
    /// Hard cap on hits per key inside `per_key_window`. The `max`-th + 1 hit is
    /// rejected as `per_key_cap_reason`. `None` disables the per-key cap — the
    /// login-link case, where the 60 s min-gap alone throttles and no per-key
    /// cap reason is ever emitted (the degenerate "single last-Instant" shape).
    pub per_key_max: Option<usize>,
    /// Reason tag returned when `per_key_max` is hit (door: `"per_user_cap"`).
    /// Ignored when `per_key_max` is `None`.
    pub per_key_cap_reason: &'static str,
    /// Memory horizon for a key. A key is evicted from the map once it has NO
    /// live hit inside `per_key_window` AND its newest hit is at least this old.
    /// Set EQUAL to `per_key_window` to drop a key the moment its hits expire
    /// (door); set WIDER to keep the key observable past its decision window
    /// (login: 120 s memory vs a 60 s decision window).
    pub key_memory: Duration,
    /// Sliding window for the global counter.
    pub global_window: Duration,
    /// Global cap across all keys inside `global_window`. The `max`-th + 1 hit
    /// is rejected as `global_cap`.
    pub global_max: usize,
}

/// Per-key throttle state.
struct KeyBucket {
    /// Accepted hit instants still inside `per_key_window`, oldest at the front.
    hits: VecDeque<Instant>,
    /// The most-recent accepted hit — retained even after `hits` is pruned so
    /// key eviction can use the (possibly wider) `key_memory` horizon.
    last: Instant,
}

/// Generic sliding-window limiter: a per-key window (min-gap + optional cap)
/// under a single global cap, with quiet-key eviction.
pub struct SlidingWindowLimiter<K: Eq + Hash + Clone> {
    per_key: HashMap<K, KeyBucket>,
    global: VecDeque<Instant>,
    config: RateLimitConfig,
}

impl<K: Eq + Hash + Clone> SlidingWindowLimiter<K> {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            per_key: HashMap::new(),
            global: VecDeque::new(),
            config,
        }
    }

    /// Number of keys currently tracked in the per-key map. Exposed so tests can
    /// assert the map stays bounded (i.e. that key eviction actually fires).
    pub fn tracked_keys(&self) -> usize {
        self.per_key.len()
    }

    /// Returns `Ok` and records the hit if allowed; `Err(reason)` otherwise.
    /// Production shim over `check_and_record_at` using the real clock.
    pub fn check_and_record(&mut self, key: K) -> Result<(), &'static str> {
        self.check_and_record_at(key, Instant::now())
    }

    /// Same as `check_and_record` but takes the current `Instant` so unit tests
    /// can simulate elapsed time without sleeping.
    pub fn check_and_record_at(&mut self, key: K, now: Instant) -> Result<(), &'static str> {
        let cfg = self.config; // `Copy` — detaches the borrow so fields stay mutable.

        // 1. Prune the global window (strict `>` keeps a boundary-aged entry).
        while let Some(&front) = self.global.front() {
            if now.duration_since(front) > cfg.global_window {
                self.global.pop_front();
            } else {
                break;
            }
        }

        // 2. Prune every key's decision window and evict quiet keys. A key is
        //    kept while it still has a live hit inside `per_key_window` OR its
        //    last activity is within `key_memory`. This is the memory-hygiene
        //    pass the door limiter was missing (#166): with door's
        //    `key_memory == per_key_window` a key drops the moment its hits
        //    expire; login's wider `key_memory` keeps the key for the full
        //    memory window (matching the old `retain(age < 120s)`).
        self.per_key.retain(|_, bucket| {
            while let Some(&front) = bucket.hits.front() {
                if now.duration_since(front) > cfg.per_key_window {
                    bucket.hits.pop_front();
                } else {
                    break;
                }
            }
            !bucket.hits.is_empty() || now.duration_since(bucket.last) < cfg.key_memory
        });

        // 3. Global cap — checked BEFORE any per-key gate (both originals do),
        //    so a globally-throttled hit reports `global_cap`.
        if self.global.len() >= cfg.global_max {
            return Err(REASON_GLOBAL_CAP);
        }

        // 4. Per-key gates. Read-only: a REJECTED hit never creates a key. The
        //    key's `hits` were already pruned to `per_key_window` in step 2.
        if let Some(bucket) = self.per_key.get(&key) {
            if let Some(gap) = cfg.per_key_min_gap
                && let Some(&last_hit) = bucket.hits.back()
                && now.duration_since(last_hit) < gap
            {
                return Err(REASON_TOO_FAST);
            }
            if let Some(max) = cfg.per_key_max
                && bucket.hits.len() >= max
            {
                return Err(cfg.per_key_cap_reason);
            }
        }

        // 5. Record.
        let bucket = self.per_key.entry(key).or_insert_with(|| KeyBucket {
            hits: VecDeque::new(),
            last: now,
        });
        bucket.hits.push_back(now);
        bucket.last = now;
        self.global.push_back(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The door route's config (per-user 10 s min-gap + 5/60 s cap, global
    /// 30/60 s). Mirrors `door::RateLimiter::new`.
    fn door_cfg() -> RateLimitConfig {
        RateLimitConfig {
            per_key_window: Duration::from_secs(60),
            per_key_min_gap: Some(Duration::from_secs(10)),
            per_key_max: Some(5),
            per_key_cap_reason: "per_user_cap",
            key_memory: Duration::from_secs(60),
            global_window: Duration::from_secs(60),
            global_max: 30,
        }
    }

    /// The login-link config (per-email 60 s min-interval, NO per-key cap, 120 s
    /// memory, global 10/60 s). Mirrors `auth::LoginLinkRateLimiter::new`.
    fn login_cfg() -> RateLimitConfig {
        RateLimitConfig {
            per_key_window: Duration::from_secs(60),
            per_key_min_gap: Some(Duration::from_secs(60)),
            per_key_max: None,
            per_key_cap_reason: "",
            key_memory: Duration::from_secs(120),
            global_window: Duration::from_secs(60),
            global_max: 10,
        }
    }

    /// Door leak fix (#166): a per-key entry is evicted once its hits age out of
    /// the window, so the map does not grow unbounded across distinct users —
    /// the emptied-entry growth the door limiter never guarded against.
    #[test]
    fn per_key_entry_is_evicted_after_window() {
        let mut rl = SlidingWindowLimiter::new(door_cfg());
        let t0 = Instant::now();
        for uid in 1..=5 {
            rl.check_and_record_at(uid, t0).unwrap();
        }
        assert_eq!(rl.tracked_keys(), 5, "five users tracked while fresh");
        // A 6th user presses well after the 60 s window: the five stale keys
        // must be evicted by the sweep.
        rl.check_and_record_at(6, t0 + Duration::from_secs(90))
            .unwrap();
        assert_eq!(
            rl.tracked_keys(),
            1,
            "stale per-user keys must be evicted, leaving only the recent presser"
        );
    }

    /// A hit aged exactly at the window boundary is KEPT (still counts), proving
    /// eviction uses `age > window`, not `>=` — the same strict boundary the
    /// door cap tests lock down. The key is retained, not dropped.
    #[test]
    fn per_key_entry_kept_at_exactly_window_boundary() {
        let mut rl = SlidingWindowLimiter::new(door_cfg());
        let t0 = Instant::now();
        rl.check_and_record_at(1, t0).unwrap();
        rl.check_and_record_at(1, t0 + Duration::from_secs(60))
            .unwrap();
        assert_eq!(
            rl.tracked_keys(),
            1,
            "key at the exact boundary is retained"
        );
    }

    /// The degenerate `per_key_max = None` config (login-link) never returns a
    /// per-key cap: the min-gap alone throttles, exactly like the old single-
    /// Instant limiter. Ten sends spaced past the gap all succeed; a fast send
    /// is `too_fast`, never a cap reason.
    #[test]
    fn no_per_key_cap_when_max_is_none() {
        let mut rl = SlidingWindowLimiter::new(login_cfg());
        let t0 = Instant::now();
        for i in 0..10 {
            rl.check_and_record_at("a@x.com".to_string(), t0 + Duration::from_secs(60 * i))
                .unwrap_or_else(|e| panic!("send #{i} spaced 60 s apart must succeed, got {e}"));
        }
        assert_eq!(
            rl.check_and_record_at("a@x.com".to_string(), t0 + Duration::from_secs(60 * 9 + 30)),
            Err("too_fast"),
            "a fast send must be too_fast — never a per-key cap when max is None"
        );
    }

    /// Global cap is evaluated BEFORE the per-key gate: a globally-capped hit
    /// reports `global_cap`, not a per-key reason. Shared ordering guarantee.
    #[test]
    fn global_cap_precedes_per_key_gate() {
        let mut rl = SlidingWindowLimiter::new(door_cfg());
        let t0 = Instant::now();
        for uid in 1..=30 {
            rl.check_and_record_at(uid, t0 + Duration::from_millis(uid as u64))
                .unwrap();
        }
        assert_eq!(
            rl.check_and_record_at(31, t0 + Duration::from_millis(100)),
            Err("global_cap"),
            "31st distinct user inside the window hits the global cap first"
        );
    }

    /// A key whose hits have aged out of the DECISION window but whose last
    /// activity is still inside the WIDER `key_memory` window is RETAINED — the
    /// login-link "observable past the decision window" property (the entry no
    /// longer throttles but is kept until the memory horizon). Locks the retain
    /// predicate's `!hits.is_empty() OR last-within-memory` shape.
    #[test]
    fn key_retained_between_decision_and_memory_window() {
        let mut rl = SlidingWindowLimiter::new(login_cfg());
        let t0 = Instant::now();
        rl.check_and_record_at("a@x.com".to_string(), t0).unwrap();
        // A different key at t0 + 90 s: a@'s only hit is now 90 s old — past the
        // 60 s decision window (its hits deque empties) but within the 120 s
        // memory window, so a@ must still be tracked.
        rl.check_and_record_at("b@x.com".to_string(), t0 + Duration::from_secs(90))
            .unwrap();
        assert_eq!(
            rl.tracked_keys(),
            2,
            "a key past its decision window but within memory must stay tracked"
        );
    }
}
