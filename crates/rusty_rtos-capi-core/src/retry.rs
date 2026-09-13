//! `Wait::Blocked` as the C sees it.
//!
//! The Kairos kernel is stackless: a blocking call keeps its locals in the
//! TCB and answers `Wait::Blocked`, whose own doc comment reads "call again
//! when this task next runs". A C task cannot work that way -- `vTaskDelay`
//! must return LATER, with its locals intact -- so the seam turns the enum
//! back into a blocking call: a loop around a yield, on a real stack the
//! port provides.
//!
//! That loop is where the seam's worst failure lives, so the bookkeeping
//! for it is here with its tests.

/// How many times a retry loop may go round without progress before it is
//  called a hang rather than a wait.
///
/// A genuinely blocking call costs one iteration per WAKEUP, because the
/// kernel does not schedule a task whose wait is still unsatisfied -- so a
/// real block reaches single digits even across a long timeout. Reaching a
/// million means the loop is spinning, not waiting.
pub const SPIN_LIMIT: u32 = 1_000_000;

/// The state of one retry loop.
///
/// # Why this exists at all
///
/// A retry loop that makes no progress is indistinguishable, from outside,
/// from a deadlock, a starved monitor, or a fault loop: the run simply
/// stops. Worse, if the loop was entered inside a critical section then the
/// tick that would unblock it can never fire, so the hang is self-sealing
/// and even a heartbeat goes quiet.
///
/// The guard costs one increment per blocked iteration and converts all of
/// that into one line naming the function that stopped.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpinGuard {
    spins: u32,
    limit: u32,
}

impl SpinGuard {
    /// A guard at the default [`SPIN_LIMIT`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            spins: 0,
            limit: SPIN_LIMIT,
        }
    }

    /// A guard at a chosen limit, for a test or a tighter caller.
    #[must_use]
    pub const fn with_limit(limit: u32) -> Self {
        Self { spins: 0, limit }
    }

    /// Count one iteration that made no progress. `true` means the loop
    /// has passed its limit and the caller should report a hang.
    pub fn blocked(&mut self) -> bool {
        self.spins = self.spins.saturating_add(1);
        self.spins >= self.limit
    }

    /// Iterations counted so far.
    #[must_use]
    pub const fn spins(self) -> u32 {
        self.spins
    }
}

/// What a blocked call should do next, given the block time the C passed.
///
/// `xTicksToWait == 0` is "do not block", and the C expects the failure
/// answer immediately -- yielding there would turn a poll into a spin,
/// which is how a non-blocking demo task starves everything beneath it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocked {
    /// Give the C its failure answer now.
    GiveUp,
    /// Leave the CPU and try again when this task next runs.
    Yield,
}

/// Decide between the two, from the `xTicksToWait` the C passed.
#[must_use]
pub const fn on_blocked(ticks_to_wait: u64) -> Blocked {
    if ticks_to_wait == 0 {
        Blocked::GiveUp
    } else {
        Blocked::Yield
    }
}

#[cfg(test)]
// The house lint policy (H-15..H-18) bans panicking APIs and bare
// indexing on every path. A test IS the path where a panic is the
// report, so tests opt out per file, as every Kairos package does.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_block_time_gives_up_rather_than_spinning() {
        assert_eq!(on_blocked(0), Blocked::GiveUp);
    }

    #[test]
    fn any_block_time_at_all_yields() {
        assert_eq!(on_blocked(1), Blocked::Yield);
        assert_eq!(on_blocked(u64::MAX), Blocked::Yield);
    }

    #[test]
    fn the_guard_fires_exactly_at_its_limit() {
        let mut g = SpinGuard::with_limit(3);
        assert!(!g.blocked());
        assert!(!g.blocked());
        assert!(g.blocked());
        assert_eq!(g.spins(), 3);
    }

    #[test]
    fn a_real_block_never_reaches_the_default_limit() {
        // A wakeup per iteration: even a thousand of them is four orders
        // of magnitude short of calling it a spin.
        let mut g = SpinGuard::new();
        for _ in 0..1_000 {
            assert!(!g.blocked());
        }
    }

    #[test]
    fn the_counter_saturates_rather_than_wrapping_back_under_the_limit() {
        let mut g = SpinGuard::with_limit(2);
        for _ in 0..10 {
            assert!(g.blocked() || g.spins() < 2);
        }
        assert_eq!(g.spins(), 10);
    }
}
