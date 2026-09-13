# rusty_rtos-capi

[![crates.io](https://img.shields.io/crates/v/rusty_rtos-capi.svg)](https://crates.io/crates/rusty_rtos-capi)
[![docs.rs](https://docs.rs/rusty_rtos-capi/badge.svg)](https://docs.rs/rusty_rtos-capi)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The FreeRTOS C ABI over the Kairos kernel: xTaskCreate, xQueueSend, xSemaphoreTake, xTimerCreate and the rest as extern C symbols with generated FreeRTOS.h-compatible headers, so a C program relinks and the unmodified C demo tasks pass.

Part of **Kairos**, the Remade-With-Rust programme that rebuilds the FreeRTOS
portfolio in memory-safe Rust, as independent packages that expose the API a
FreeRTOS developer already knows and prove every scheduling decision against
the C kernel's own trace.

- This package's plan: [docs/plans/rusty_rtos-capi.md](docs/plans/rusty_rtos-capi.md)
- Every number: [docs/LEDGER.md](docs/LEDGER.md)
- The family plan: Kairos `docs/plans/rtos-mission.md` (umbrella repo)

**Claims discipline:** this README makes no performance or capability claim that
is not backed by a test, a benchmark ledger entry, or a kill test recorded in the
plan. "Scaffold" means scaffold. "Sim only" means the sim port; "builds, not
flashed" means no chip has run it.

## Status

**Both halves of the kill test pass.** **25 unmodified C demo files from the
pinned FreeRTOS distribution** run together against the Kairos kernel — on
QEMU Cortex-M3, on Windows threads and on Linux pthreads — with every
verdict taken from the demo file's own checker rather than from ours. A
twenty-sixth, `MessageBufferAMP.c`, runs in a binary of its own because its
`sbSEND_COMPLETED` override is process-wide.

Nothing about the C is patched, wrapped or regenerated. The files come
straight out of the pinned `oracle/` checkout and are compiled against the
oracle's own `FreeRTOS.h`, `task.h` and `queue.h`. The only file either cell
hands the C side is a `FreeRTOSConfig.h`, which every FreeRTOS application
supplies.

| | each alone | together | together, sustained checking |
|---|---:|---|---|
| QEMU Cortex-M3 | **25 / 25** | **25 / 25** | **25 / 25** |
| host, Windows threads | **25 / 25** | **25 / 25** | **25 / 25** |
| host, Linux pthreads | **25 / 25** | **25 / 25** | **25 / 25** |
| `--features amp`, its own binary | **1 / 1** | — | — |

`flash.c` also runs and is **not** in those numbers: it exports a start
function and no checker, so there is no verdict of its own to report and
none is invented. It is listed separately, with the LED counts that are the
only evidence it ran — and those are ours, not the demo's.

| | |
|---|---:|
| `extern "C"` symbols, derived with `llvm-nm -u` rather than chosen | 87 |
| defects found and fixed | 17 |

It was 89 and is now 87, because `strcmp` and `strncmp` were being claimed as
part of the ABI. They are defined in the seam for one reason — a bare-metal
cell has no libc to link — and FreeRTOS does not export them, so declaring
them in `kairos_capi.h` was a claim about this ABI's surface that was not
true. The deriver now stops at the seam's own `tiny libc` banner, which also
kept `sprintf` out: our three-argument stand-in would have CONFLICTED with
the real `<stdio.h>` in the header gate's translation unit.

**What "sustained checking" means, because the weaker question is easy to
pass.** The demos ship a check task that asks each checker every 10,000
ticks and LATCHES failures. Asking once at the end — the obvious way to
write this harness — is the weakest form of that question, and moving to the
demos' own cadence found seven further defects, three of which the weak form
had been hiding on both ports. Both cells are now clean under it, and the
M3 result is reproducible across runs.

The last of those seven was not a defect in this code. `StreamBufferDemo`'s
trigger-level test asserts on an exact byte count with no margin, and on
QEMU 2 of ~290 receives blocked one tick longer than asked — never on the
host. It was CPU starvation in the emulated cell: sixty tasks and a tick
hook that runs every demo's ISR half every tick did not fit in 20,000
cycles. At 80,000 the byte histogram becomes identical to the host's and the
count is zero. The cell's `FreeRTOSConfig.h` carries the 20/40/80 MHz series
that established it. The demo ships
`configSTREAM_BUFFER_TRIGGER_LEVEL_TEST_MARGIN` to widen that assertion and
we deliberately do not set it — no FreeRTOS demo project does, and it would
have hidden the measurement rather than made it.

The host cell runs on **Windows and on Linux**, on each platform's own
threads: `SuspendThread` on Windows, and on Unix `pthread_kill` with a
handler that parks the target in `sigsuspend`, because Unix has no call
that stops another thread from outside. Same 21 files, same seam, 21/21
under sustained checking on both.

**Why not all 34, and it is a property of the demo files.** Four of them
come in mutually exclusive pairs or need a binary to themselves —
`flop.c`/`sp_flop.c` and `comtest.c`/`comtest_strings.c` each define the
same symbols, `MessageBufferAMP.c`'s `sbSEND_COMPLETED` is process-wide, and
`flash_timer.c` starts timers in the window `TimerDemo.c` deliberately fills.
Two more (`flash.c`, `flash_timer.c`) ship no checker at all, so they can
run but can never be graded. `IntQueue.c` needs a board-specific header a
demo *project* supplies. So **no single binary can run all 34**, and the
ceiling with a verdict of its own is 31 across at least three binaries.
The arithmetic, and the three decisions that would close the rest
(co-routines, static allocation, `IntQueue`), are in
[docs/plans/rusty_rtos-capi.md](docs/plans/rusty_rtos-capi.md) §4b.

Every run is in [docs/LEDGER.md](docs/LEDGER.md).

## What it is

- A pure-Rust remake of the corresponding FreeRTOS component. Same job, same
  names, same semantics, new code, permissive licence, `forbid(unsafe)` in
  the core.
- Arch-agnostic: the core crate is `no_std` (+ `alloc`) and knows nothing about
  a CPU, an allocator or an operating system. Ports and backends are thin,
  feature-gated WRAP crates.

## What it is not

- Not a fork of FreeRTOS and not a binding to it. The C kernel is the
  **oracle** this package is measured against, never a dependency.
- Not a rewrite of a radio blob, a ROM or a vendor driver. Where silicon must
  be touched, a port crate **wraps** `cortex-m-rt` / `riscv-rt` / `esp-hal`
  and says so.

## Layout

```text
crates/rusty_rtos-capi          facade: re-exports + prelude; the crate you depend on
crates/rusty_rtos-capi-core     no_std (+ alloc); forbid(unsafe); types, traits, algorithms
firmware/                per-chip example projects, excluded from the workspace
docs/plans/              this package's plan and its hardening audit
docs/LEDGER.md           every number, with its method line
```

## Build

```sh
cargo test --workspace                                   # host: the tests
cargo check -p rusty_rtos-capi-core --no-default-features \
  --target thumbv7em-none-eabihf                         # Cortex-M4F class, no alloc
cargo check -p rusty_rtos-capi-core --no-default-features --features alloc \
  --target riscv32imac-unknown-none-elf                  # ESP32-C6 class, with alloc
```

CI holds the core to `thumbv7em-none-eabihf`, `thumbv8m.main-none-eabihf`,
`riscv32imac-unknown-none-elf` and `riscv32imafc-unknown-none-elf`, with and
without `alloc`, plus `cargo deny check`. Firmware examples (Xtensa needs the
esp toolchain; Cortex-M and RISC-V work on stable) are built from their own
directories under `firmware/`.

## License

MIT OR Apache-2.0, at your option. FreeRTOS is MIT-licensed by Amazon.com,
Inc. or its affiliates; this crate remakes its API and behaviour from the
published sources and links no FreeRTOS code.

---

<!-- HARDENING-TABLE:BEGIN generated by use-protection-please — edit docs/plans/use-protection-please.md, not this block -->
## Hardening status

**Tier** critical-path · **Audited** 2026-09-09 (survey) · **v1.0.0 gates** 6/16 · [Full checklist](docs/plans/use-protection-please.md)

`████░░░░░░░░░░░░░░░░` **22%** &nbsp;·&nbsp; 8 Completed · 0 Scheduled · 28 Incomplete · 19 N/A

| Phase | ✅ Completed | 🗓 Scheduled | ⬜ Incomplete | · N/A |
|---|--:|--:|--:|--:|
| 0 — Threat modeling | 0 | 0 | 2 | 0 |
| 1 — Toolchain | 2 | 0 | 2 | 0 |
| 2 — Supply chain | 2 | 0 | 6 | 0 |
| 3 — Code level | 3 | 0 | 4 | 0 |
| 4 — Static analysis | 0 | 0 | 1 | 0 |
| 5 — Dynamic analysis | 0 | 0 | 3 | 0 |
| 6 — Fuzzing and properties | 0 | 0 | 4 | 0 |
| 7 — Formal verification | 0 | 0 | 1 | 0 |
| 8 — Build and binary | 0 | 0 | 1 | 1 |
| 9 — Runtime privilege | 0 | 0 | 0 | 1 |
| 10 — Cryptography | 0 | 0 | 0 | 3 |
| 11 — CI/CD, release, and operations | 1 | 0 | 4 | 0 |
| 12 — Compliance controls | 0 | 0 | 0 | 14 |
| **Total** | **8** | **0** | **28** | **19** |

**Architect** — [Tim Almond](https://github.com/Ttimmahlax) — accountable for this unit's security design; rendered
<!-- HARDENING-TABLE:END -->
