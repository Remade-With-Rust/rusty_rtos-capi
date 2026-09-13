//! The C's scalar types and constants, written down beside where they came
//! from.
//!
//! Getting one of these wrong is an ABI bug that no compiler can see: both
//! sides build cleanly and disagree at runtime about what a word means. So
//! each name here carries the header and the line of reasoning it came
//! from, and the two configurations that must agree are named as such.
//!
//! These are the ARM_CM3 / 32-bit answers. A 64-bit host port answers
//! differently for `BaseType_t`, which is why they are here rather than
//! spelled inline at eighty call sites.

/// `portBASE_TYPE`, which is a **port** fact and not a constant.
///
/// `portable/GCC/ARM_CM3/portmacro.h` says `long`, 32 bits on that target.
/// `portable/MSVC-MingW/portmacro.h` says `long long` under `_WIN64` and
/// `long` otherwise, and the Posix port agrees with the 64-bit answer on a
/// 64-bit Unix. So this follows the pointer width, which is the same test
/// those headers make.
///
/// It has to be got right on BOTH sides of the seam or the two disagree
/// about the width of a return value while compiling cleanly, which is the
/// one class of ABI bug no compiler can see. The generated header emits
/// the same conditional, so a C program and this crate reach the same
/// answer from the same question.
#[cfg(target_pointer_width = "64")]
#[allow(non_camel_case_types)]
pub type BaseType_t = i64;

/// `portBASE_TYPE` on a 32-bit target: `long`.
#[cfg(not(target_pointer_width = "64"))]
#[allow(non_camel_case_types)]
pub type BaseType_t = i32;

/// The unsigned twin of [`BaseType_t`].
#[cfg(target_pointer_width = "64")]
#[allow(non_camel_case_types)]
pub type UBaseType_t = u64;

/// The unsigned twin of [`BaseType_t`], 32-bit target.
#[cfg(not(target_pointer_width = "64"))]
#[allow(non_camel_case_types)]
pub type UBaseType_t = u32;

/// `TickType_t`, when `configTICK_TYPE_WIDTH_IN_BITS` is
/// `TICK_TYPE_WIDTH_32_BITS`. A config that says 16 changes this, and the
/// C and the Rust must say the same thing or every timeout is wrong by a
/// factor of 65,536.
#[allow(non_camel_case_types)]
pub type TickType_t = u32;

/// `configSTACK_DEPTH_TYPE` defaults to `StackType_t`, which is the port's
/// stack word: `uint32_t` on ARM_CM3 and `size_t` on the host ports. It
/// counts WORDS, not bytes -- `configMINIMAL_STACK_SIZE` of 256 is a
/// kilobyte on a 32-bit target and two on a 64-bit one, which is a reason
/// to write stack sizes in terms of the macro rather than in numbers.
#[allow(non_camel_case_types)]
pub type StackDepth_t = usize;

/// `sizeof( configMESSAGE_BUFFER_LENGTH_TYPE )`: the bytes a message
/// buffer spends on each message's length prefix.
///
/// `FreeRTOS.h` defaults that type to `size_t`, so this is the target's
/// pointer width — **not a number to write down**. It is in the arithmetic
/// of every message send and `xMessageBufferSpaceAvailable`, so a cell that
/// guesses it wrong reports the wrong free space and the C notices:
/// `MessageBufferDemo.c:262` compares the two directly, and a four-byte
/// disagreement fails it on the first message.
///
/// Derived here rather than declared per cell because the kernel's
/// `Config` trait defaults it to 4 — right for a 32-bit chip, wrong by
/// four bytes on a 64-bit host — and a default that is right on one of two
/// ports is the shape of every two-halves bug this ABI has had.
pub const MESSAGE_LENGTH_BYTES: usize = core::mem::size_of::<usize>();

/// `projdefs.h`: `#define pdPASS ( pdTRUE )`, `pdTRUE` being 1.
pub const PD_PASS: BaseType_t = 1;
/// `#define pdFAIL ( pdFALSE )`.
pub const PD_FAIL: BaseType_t = 0;
/// `#define pdTRUE ( ( BaseType_t ) 1 )`.
pub const PD_TRUE: BaseType_t = 1;
/// `#define pdFALSE ( ( BaseType_t ) 0 )`.
pub const PD_FALSE: BaseType_t = 0;
/// `queue.h`: `#define errQUEUE_FULL ( ( BaseType_t ) 0 )`. The same value
/// as `pdFAIL`, named separately because the call sites mean different
/// things by it.
pub const ERR_QUEUE_FULL: BaseType_t = 0;
/// `projdefs.h`: `#define errQUEUE_EMPTY ( ( BaseType_t ) 0 )`.
pub const ERR_QUEUE_EMPTY: BaseType_t = 0;

/// A `BaseType_t` from a Rust `bool`, the way the C spells it.
#[must_use]
pub const fn pd(b: bool) -> BaseType_t {
    if b { PD_PASS } else { PD_FAIL }
}

/// Which end of the queue a send writes to.
///
/// `xQueueSend`, `xQueueSendToBack`, `xQueueSendToFront` and
/// `xQueueOverwrite` are all macros over `xQueueGenericSend`, separated
/// only by this argument. A seam that handles two of the three turns the
/// third into a DIFFERENT OPERATION with no error anywhere -- overwrite
/// silently became append, and `QueueSet.c:1138` is what finally caught
/// it, a thousand lines from the call that did it.
///
/// That is why this is an enum with an exhaustive decode rather than a
/// pair of `if`s: the third case cannot be forgotten, because there is no
/// fall-through to forget it into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CopyPosition {
    /// `queueSEND_TO_BACK` (0).
    Back,
    /// `queueSEND_TO_FRONT` (1).
    Front,
    /// `queueOVERWRITE` (2): write to a length-one queue whether or not it
    /// already holds something.
    Overwrite,
}

impl CopyPosition {
    /// `queueSEND_TO_BACK`, from `queue.c`.
    pub const BACK: BaseType_t = 0;
    /// `queueSEND_TO_FRONT`.
    pub const FRONT: BaseType_t = 1;
    /// `queueOVERWRITE`.
    pub const OVERWRITE: BaseType_t = 2;

    /// Decode `xCopyPosition`. `None` for a value the C never sends,
    /// which a caller should refuse rather than guess at.
    #[must_use]
    pub const fn decode(x_copy_position: BaseType_t) -> Option<Self> {
        match x_copy_position {
            Self::BACK => Some(Self::Back),
            Self::FRONT => Some(Self::Front),
            Self::OVERWRITE => Some(Self::Overwrite),
            _ => None,
        }
    }

    /// The `xCopyPosition` the C would have passed for this.
    #[must_use]
    pub const fn encode(self) -> BaseType_t {
        match self {
            Self::Back => Self::BACK,
            Self::Front => Self::FRONT,
            Self::Overwrite => Self::OVERWRITE,
        }
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

    /// The widths are the PORT's, and the test says which port it is
    /// asking about rather than asserting one number everywhere.
    #[test]
    fn the_scalar_widths_are_the_port_s() {
        let word = core::mem::size_of::<usize>();
        assert_eq!(
            core::mem::size_of::<BaseType_t>(),
            word,
            "portBASE_TYPE follows the pointer width, as portmacro.h does"
        );
        assert_eq!(core::mem::size_of::<UBaseType_t>(), word);
        assert_eq!(core::mem::size_of::<StackDepth_t>(), word);
        // `configTICK_TYPE_WIDTH_IN_BITS` is 32 in every Kairos config, on
        // every target. It is a CONFIG fact, not a port one, which is why
        // this line has no `cfg` on it.
        assert_eq!(core::mem::size_of::<TickType_t>(), 4);
        // `size_t`, so it follows the pointer width the way the C's
        // `sizeof( configMESSAGE_BUFFER_LENGTH_TYPE )` does.
        assert_eq!(
            MESSAGE_LENGTH_BYTES,
            if cfg!(target_pointer_width = "64") {
                8
            } else {
                4
            }
        );
    }

    #[test]
    fn the_signed_and_unsigned_halves_are_the_same_width() {
        assert_eq!(
            core::mem::size_of::<BaseType_t>(),
            core::mem::size_of::<UBaseType_t>()
        );
    }

    #[test]
    fn pd_pass_is_one_and_pd_fail_is_zero() {
        assert_eq!(pd(true), PD_PASS);
        assert_eq!(pd(false), PD_FAIL);
        assert_eq!(PD_FAIL, ERR_QUEUE_FULL);
    }

    /// K6 defect 3. Overwrite fell through to `Back` and every
    /// `xQueueOverwrite` silently became an append.
    #[test]
    fn all_three_copy_positions_decode_and_overwrite_is_not_back() {
        assert_eq!(CopyPosition::decode(0), Some(CopyPosition::Back));
        assert_eq!(CopyPosition::decode(1), Some(CopyPosition::Front));
        assert_eq!(CopyPosition::decode(2), Some(CopyPosition::Overwrite));
        assert_ne!(CopyPosition::Overwrite, CopyPosition::Back);
    }

    #[test]
    fn a_position_the_c_never_sends_is_refused_not_guessed() {
        assert_eq!(CopyPosition::decode(3), None);
        assert_eq!(CopyPosition::decode(-1), None);
    }

    #[test]
    fn decode_and_encode_round_trip() {
        for p in [
            CopyPosition::Back,
            CopyPosition::Front,
            CopyPosition::Overwrite,
        ] {
            assert_eq!(CopyPosition::decode(p.encode()), Some(p));
        }
    }
}
