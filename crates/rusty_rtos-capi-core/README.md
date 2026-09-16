# rusty_rtos-capi-core

[![Remade With Rust](https://img.shields.io/badge/Remade%20With-Rust-000?logo=rust&logoColor=fff)](https://github.com/remade-with-rust)
[![By Mata Network](https://img.shields.io/badge/by-Mata%20Network-5b2be0)](https://www.mata.network)
[![crates.io](https://img.shields.io/crates/v/rusty_rtos-capi-core.svg)](https://crates.io/crates/rusty_rtos-capi-core)
[![docs.rs](https://docs.rs/rusty_rtos-capi-core/badge.svg)](https://docs.rs/rusty_rtos-capi-core)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The **rules** of the Kairos C ABI: the C's type widths, the handle codec, the
C string reader, the copy positions, the retry protocol, and the ABI itself as a
data table that generates the header. `#![forbid(unsafe_code)]`, 38 tests.

- **What is in here**: `ctypes` (`BaseType_t`, `UBaseType_t`, `TickType_t`,
  `StackDepth_t` and the message-buffer length prefix — each following the
  target the way `portmacro.h` makes it follow), the handle encode/decode with
  its null guard, and `symbols`, which is the 87-symbol ABI as data plus the
  writer that generates `kairos_capi.h`.
- **Why the rules are a crate**: five hand-written copies of the handle encode
  once lived in the seam, and only ONE of them had the null-handle guard that
  `GenQTest.c:564` had already found. A rule written five times is a rule fixed
  once.

**Known gaps.** This crate holds the rules, **not the ABI**: the seam that
defines the 87 `extern "C"` symbols is not yet a library crate, so depending on
this does not give you a linkable FreeRTOS ABI.

## Conformance

The 87 symbols are **derived, not chosen**: the unmodified demo files were
compiled and `llvm-nm -u` was asked what they wanted — 108 undefined symbols
across 33 files, of which 11 are compiler helpers and 6 are board drivers.
`tools/derive_symbols.py` re-derives the table from the seam and `--check`
reports drift.

26 demo files run against the kernel through this ABI, on QEMU Cortex-M3 and on
a host under Windows threads and Linux pthreads. Full tables in the
[repository README](https://github.com/Remade-With-Rust/rusty_rtos-capi#conformance).

## Using it

```rust
use rusty_rtos_capi_core::ctypes::{BaseType_t, TickType_t, PD_PASS};

// The C's widths follow the target, exactly as `portmacro.h` makes them:
// `BaseType_t` is 32 bits on a Cortex-M3 and 64 on a host, and a demo entry
// point declared `u32` instead handed a task priority 0 rather than 3.
assert_eq!(core::mem::size_of::<TickType_t>(), 4);
assert_eq!(PD_PASS, 1 as BaseType_t);
```

## Performance

No rows: this crate is rules and tables. The measured numbers belong to the
kernel and the port.

## Portability

`no_std` on host, `thumbv7m-none-eabi`, `riscv32imac-unknown-none-elf` and
`xtensa-esp32s3-none-elf`. The type widths change with the target on purpose —
that is what the crate is for.

## Part of Remade With Rust

This crate is part of **[Kairos](https://github.com/Remade-With-Rust/kairos)** —
FreeRTOS remade in memory-safe Rust, as independent packages that expose the API
a FreeRTOS developer already knows and prove every scheduling decision against
the C kernel's own trace. The family:
[`rusty_rtos_core`](https://crates.io/crates/rusty_rtos_core),
[`rusty_rtos_kernel`](https://crates.io/crates/rusty_rtos_kernel),
[`rusty_rtos_port`](https://crates.io/crates/rusty_rtos_port),
[`rusty_rtos_heap`](https://crates.io/crates/rusty_rtos_heap). Also check out the
rest of **[github.com/remade-with-rust](https://github.com/remade-with-rust)**.

## About Mata Network

<!-- ORG BOILERPLATE — keep identical across repos -->

[Mata Network](https://www.mata.network) builds sovereign, self-hostable
infrastructure. **Remade With Rust** is our open-source home for the
permissively-licensed building blocks that work depends on.

<!-- /ORG BOILERPLATE -->

## License

MIT OR Apache-2.0, at your option. FreeRTOS is MIT-licensed by Amazon.com, Inc.
or its affiliates; this crate remakes its API and behaviour from the published
sources and links no FreeRTOS code.
