//! Age and freshness of a state component at the decision time.
//!
//! The legacy contract answered freshness through host queries; here it is computed from the
//! component's own [`ComponentMeta`] and the decision time (`ctx.runtime().now()`), so it needs
//! no host call and gives the same answer when a decision is re-run.

use chrono::{DateTime, Duration, Utc};

use crate::state::{ComponentAuthority, ComponentMeta};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FreshnessStatus {
    /// Updated no longer ago than the caller's limit.
    Fresh,
    /// Updated longer ago than the caller's limit.
    Stale,
    /// The host has no update time for the component.
    Missing,
}

/// How old one component is. `status` depends only on time; `authority` and `refresh_error`
/// are the host's own view of the component and are reported beside it, not folded into it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Freshness<'a> {
    pub status: FreshnessStatus,
    pub updated_at: Option<DateTime<Utc>>,
    /// Decision time less `updated_at`, never negative.
    pub age: Option<Duration>,
    pub stale_after: Duration,
    pub authority: ComponentAuthority,
    pub refresh_error: Option<&'a str>,
}

impl Freshness<'_> {
    pub const fn is_fresh(&self) -> bool {
        matches!(self.status, FreshnessStatus::Fresh)
    }
}

impl ComponentMeta {
    /// Time since the component was last updated, as of `decision_time`. An update stamped
    /// after the decision time counts as age zero. `None` when the host has no update time.
    pub fn age_at(&self, decision_time: DateTime<Utc>) -> Option<Duration> {
        self.updated_at
            .map(|updated_at| (decision_time - updated_at).max(Duration::zero()))
    }

    /// Fresh when the age is at most `stale_after`, stale when it is greater.
    pub fn freshness_at(
        &self,
        decision_time: DateTime<Utc>,
        stale_after: Duration,
    ) -> Freshness<'_> {
        let age = self.age_at(decision_time);
        Freshness {
            status: match age {
                None => FreshnessStatus::Missing,
                Some(age) if age > stale_after => FreshnessStatus::Stale,
                Some(_) => FreshnessStatus::Fresh,
            },
            updated_at: self.updated_at,
            age,
            stale_after,
            authority: self.authority,
            refresh_error: self.refresh_error.as_deref(),
        }
    }
}
