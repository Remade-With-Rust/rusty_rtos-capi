#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
//! `rusty_rtos-capi-core` — the pure heart of `rusty_rtos-capi`.
//!
//! Rules this crate lives by (from the Kairos mission plan):
//!
//! 1. `no_std` by default; `alloc` is a feature, never an assumption.
//! 2. No CPU, no registers, no allocator, no operating system. Ports and
//!    backends are separate WRAP crates.
//! 3. Every type that crosses to another Kairos package comes from
//!    `rusty_rtos_core`, so packages compose without conversions.
//! 4. Handles are indices, never pointers; nothing on a hot path allocates.
//! 5. `forbid(unsafe)`. The C kernel's trace is the oracle; the scalar path
//!    is the oracle; any faster path is gated identical against it.

#[cfg(feature = "alloc")]
extern crate alloc;

pub use rusty_rtos_core as rtos_core;

pub mod codec;
pub mod cstr;
pub mod ctypes;
pub mod retry;
pub mod symbols;

/// The names a firmware wants in scope.
pub mod prelude {
    pub use crate::codec::{handle_from_c, handle_to_c, item_from_bytes, item_to_bytes};
    pub use crate::ctypes::{
        BaseType_t, CopyPosition, ERR_QUEUE_FULL, PD_FAIL, PD_PASS, StackDepth_t, TickType_t,
        UBaseType_t, pd,
    };
    pub use crate::retry::{Blocked, SpinGuard, on_blocked};
    pub use rusty_rtos_core::prelude::*;
}

/// Crate version, for manifests and logs.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
