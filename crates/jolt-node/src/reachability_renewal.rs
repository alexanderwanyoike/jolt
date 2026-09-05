//! Keeps the local identity's direct-ingress reachability record alive while
//! the daemon runs.
//!
//! A record is only useful to remote senders until `expires_at`, so the daemon
//! must republish before then. Renewal is scheduled against wall-clock time,
//! not against elapsed ticks: after a laptop sleeps, the first tick after wake
//! sees a clock that may already be past the renewal point (or past expiry)
//! and republishes immediately instead of waiting out a stale interval.

use std::time::Duration;

/// Lifetime of each published record.
pub const REACHABILITY_TTL_SECS: u64 = 24 * 60 * 60;

/// How often the daemon checks whether renewal is due. Short relative to the
/// TTL so that a failed publication is retried well before expiry.
pub const RENEWAL_CHECK_INTERVAL: Duration = Duration::from_secs(60);

/// Wall-clock renewal schedule for a single identity's reachability record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenewalSchedule {
    ttl_secs: u64,
    // Expiry of the most recent successful publication; `None` until the first
    // publication succeeds so that a startup failure keeps renewal due.
    published_expires_at: Option<u64>,
}

impl RenewalSchedule {
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            ttl_secs,
            published_expires_at: None,
        }
    }

    /// Expiry a record published at `now` should carry.
    pub fn expires_at_for(&self, now: u64) -> u64 {
        now.saturating_add(self.ttl_secs)
    }

    /// Renewal is due once half the lifetime has elapsed, which leaves the
    /// remaining half for retries if publication or discovery is flaky.
    pub fn is_due(&self, now: u64) -> bool {
        match self.published_expires_at {
            None => true,
            Some(expires_at) => now.saturating_add(self.ttl_secs / 2) >= expires_at,
        }
    }

    pub fn record_published(&mut self, expires_at: u64) {
        self.published_expires_at = Some(expires_at);
    }
}

/// Outcome of one renewal check, so callers can log without inspecting the
/// schedule.
#[derive(Debug, PartialEq, Eq)]
pub enum RenewalOutcome<E> {
    NotDue,
    Published { expires_at: u64 },
    Failed(E),
}

/// Publishes through `publish` when the schedule says so. On failure the
/// schedule is left unchanged so the next check retries.
pub async fn renew_if_due<E, F, Fut>(
    schedule: &mut RenewalSchedule,
    now: u64,
    publish: F,
) -> RenewalOutcome<E>
where
    F: FnOnce(u64) -> Fut,
    Fut: std::future::Future<Output = Result<(), E>>,
{
    if !schedule.is_due(now) {
        return RenewalOutcome::NotDue;
    }
    let expires_at = schedule.expires_at_for(now);
    match publish(expires_at).await {
        Ok(()) => {
            schedule.record_published(expires_at);
            RenewalOutcome::Published { expires_at }
        }
        Err(err) => RenewalOutcome::Failed(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const TTL: u64 = 24 * 60 * 60;
    const T0: u64 = 1_788_000_000;

    fn recording_publisher<'a>(
        log: &'a RefCell<Vec<u64>>,
        result: Result<(), &'static str>,
    ) -> impl FnOnce(u64) -> std::future::Ready<Result<(), &'static str>> + 'a {
        move |expires_at| {
            log.borrow_mut().push(expires_at);
            std::future::ready(result)
        }
    }

    #[tokio::test]
    async fn first_check_publishes_with_full_ttl() {
        let mut schedule = RenewalSchedule::new(TTL);
        let log = RefCell::new(Vec::new());

        let outcome = renew_if_due(&mut schedule, T0, recording_publisher(&log, Ok(()))).await;

        assert_eq!(
            outcome,
            RenewalOutcome::Published {
                expires_at: T0 + TTL
            }
        );
        assert_eq!(*log.borrow(), vec![T0 + TTL]);
    }

    #[tokio::test]
    async fn fresh_record_is_not_republished_before_half_life() {
        let mut schedule = RenewalSchedule::new(TTL);
        let log = RefCell::new(Vec::new());
        renew_if_due(&mut schedule, T0, recording_publisher(&log, Ok(()))).await;

        let outcome = renew_if_due(
            &mut schedule,
            T0 + TTL / 2 - 1,
            recording_publisher(&log, Ok(())),
        )
        .await;

        assert_eq!(outcome, RenewalOutcome::NotDue);
        assert_eq!(log.borrow().len(), 1);
    }

    #[tokio::test]
    async fn record_is_renewed_at_half_life_with_a_fresh_expiry() {
        let mut schedule = RenewalSchedule::new(TTL);
        let log = RefCell::new(Vec::new());
        renew_if_due(&mut schedule, T0, recording_publisher(&log, Ok(()))).await;

        let outcome = renew_if_due(
            &mut schedule,
            T0 + TTL / 2,
            recording_publisher(&log, Ok(())),
        )
        .await;

        assert_eq!(
            outcome,
            RenewalOutcome::Published {
                expires_at: T0 + TTL / 2 + TTL
            }
        );
    }

    #[tokio::test]
    async fn clock_jump_past_expiry_renews_on_the_next_check() {
        // Laptop sleeps for two days: the first check after wake must not wait
        // for another interval.
        let mut schedule = RenewalSchedule::new(TTL);
        let log = RefCell::new(Vec::new());
        renew_if_due(&mut schedule, T0, recording_publisher(&log, Ok(()))).await;

        let woke_at = T0 + 2 * TTL;
        let outcome = renew_if_due(&mut schedule, woke_at, recording_publisher(&log, Ok(()))).await;

        assert_eq!(
            outcome,
            RenewalOutcome::Published {
                expires_at: woke_at + TTL
            }
        );
    }

    #[tokio::test]
    async fn failed_publication_stays_due_until_it_succeeds() {
        let mut schedule = RenewalSchedule::new(TTL);
        let log = RefCell::new(Vec::new());

        let failed =
            renew_if_due(&mut schedule, T0, recording_publisher(&log, Err("offline"))).await;
        assert_eq!(failed, RenewalOutcome::Failed("offline"));

        let retried = renew_if_due(&mut schedule, T0 + 60, recording_publisher(&log, Ok(()))).await;
        assert_eq!(
            retried,
            RenewalOutcome::Published {
                expires_at: T0 + 60 + TTL
            }
        );
        assert_eq!(*log.borrow(), vec![T0 + TTL, T0 + 60 + TTL]);
    }

    #[tokio::test]
    async fn failed_renewal_keeps_the_previous_expiry_and_retries() {
        let mut schedule = RenewalSchedule::new(TTL);
        let log = RefCell::new(Vec::new());
        renew_if_due(&mut schedule, T0, recording_publisher(&log, Ok(()))).await;

        let at_half = T0 + TTL / 2;
        let failed = renew_if_due(
            &mut schedule,
            at_half,
            recording_publisher(&log, Err("dht")),
        )
        .await;
        assert_eq!(failed, RenewalOutcome::Failed("dht"));

        let retried = renew_if_due(
            &mut schedule,
            at_half + 60,
            recording_publisher(&log, Ok(())),
        )
        .await;
        assert!(matches!(retried, RenewalOutcome::Published { .. }));
    }
}
