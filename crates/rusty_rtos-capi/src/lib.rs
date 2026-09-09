#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
//! `rusty_rtos-capi` — The FreeRTOS C ABI over the Kairos kernel: xTaskCreate, xQueueSend, xSemaphoreTake, xTimerCreate and the rest as extern C symbols with generated FreeRTOS.h-compatible headers, so a C program relinks and the unmodified C demo tasks pass.
//!
//! This is the facade: it re-exports the `no_std` core. Depend on this crate;
//! reach into the sub-crates only when you are building a port or a backend.
//!
//! Part of Kairos (Remade With Rust). Plan: `docs/plans/rusty_rtos-capi.md`.

pub use rusty_rtos_capi_core::*;

/// The names a firmware wants in scope.
pub mod prelude {
    pub use rusty_rtos_capi_core::prelude::*;
}
