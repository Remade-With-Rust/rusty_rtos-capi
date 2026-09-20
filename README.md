### In The Wild with 21 Active Installs

FREE RAG Converter Online -- <a href="https://RAGconverter.com">RAGconverter.com</a>

# rusty_rtos-capi

[![Remade With Rust](https://img.shields.io/badge/Remade%20With-Rust-000?logo=rust&logoColor=fff)](https://github.com/remade-with-rust)
[![By Mata Network](https://img.shields.io/badge/by-Mata%20Network-5b2be0)](https://www.mata.network)
[![crates.io](https://img.shields.io/crates/v/rusty_rtos-capi.svg)](https://crates.io/crates/rusty_rtos-capi)
[![docs.rs](https://docs.rs/rusty_rtos-capi/badge.svg)](https://docs.rs/rusty_rtos-capi)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The C ABI for Kairos. Unmodified C written against FreeRTOS, compiled by your
compiler against FreeRTOS's own headers, links and runs on a Rust kernel.
Nothing is ported, wrapped or regenerated: the C does not know it is not
talking to FreeRTOS.

- **Proven**: **26 demo files** from `Demo/Common/Minimal` in the pinned
  FreeRTOS distribution run together, 25 of them reporting a verdict from their
  own checker, on QEMU Cortex-M3 and on a host under both Windows threads and
  Linux pthreads — all clean under the demos' own 10,000-tick checking cadence
  with latching error flags.
- **The surface is derived, not designed**: the unmodified demo files were
  compiled and `llvm-nm -u` was asked what they wanted. **87 `extern "C"`
  symbols**, against an API map listing 341. Nobody gets a vote, and
  `tools/derive_symbols.py` keeps it that way.

**Known gaps.** The publishable crate here is the ABI's *rules* — the C type
widths, the handle codec, the symbol table and the generated header. The seam
that defines the 87 symbols is not yet a library crate, so **adding this crate
to a `Cargo.toml` does not yet give you a linkable ABI**; it is consumed today
by the two cells in this repo. Also absent: static allocation, co-routines, and
`IntQueue.c`, which needs a board header a demo *project* supplies.

- This package's plan: [docs/plans/rusty_rtos-capi.md](https://github.com/Remade-With-Rust/rusty_rtos-capi/blob/main/docs/plans/rusty_rtos-capi.md)
- Every number: [docs/LEDGER.md](https://github.com/Remade-With-Rust/rusty_rtos-capi/blob/main/docs/LEDGER.md)
- The family plan: Kairos [`docs/plans/rtos-mission.md`](https://github.com/Remade-With-Rust/kairos/blob/main/docs/plans/rtos-mission.md)

**Claims discipline:** this README makes no performance or capability claim that
is not backed by a test, a benchmark ledger entry, or a kill test recorded in
the plan. "Scaffold" means scaffold. "Sim only" means the sim port; "builds, not
flashed" means no chip has run it.

## Conformance

Every verdict below is the demo file's own check function, not ours.

| | each alone | together | together, sustained |
|---|---:|---|---|
| QEMU Cortex-M3 | 25/25 | **25/25** | **25/25** |
| host, Windows threads | 25/25 | **25/25** | **25/25**, 3 runs of 3 |
| host, Linux pthreads | 25/25 | **25/25** | **25/25**, 3 runs of 3 |
| `MessageBufferAMP`, its own binary | **1/1** | — | — |

`flash.c` also runs and is deliberately **not** in those numbers: it exports a
start function and no checker, so there is no verdict of its own to report and
none is invented.

**Seventeen defects, and the C, a compiler or a kill test found every one** —
including a port critical section that enabled interrupts instead of restoring
them, a timer daemon that slept beside its command queue instead of waiting on
it, and two halves of one configuration disagreeing by four bytes on a 64-bit
host. Each is a row in
[`docs/LEDGER.md`](https://github.com/Remade-With-Rust/rusty_rtos-capi/blob/main/docs/LEDGER.md).

**The header is generated and the COMPILER audits it.**
`capi/header_gate.c` compiles the generated `kairos_capi.h` beside the
oracle's real `FreeRTOS.h` / `task.h` / `queue.h` in one translation unit, so a
disagreement is "conflicting types" — it found four — and a declaration with no
definition is an undefined reference. That check was **vacuous on its first
attempt**: `--gc-sections` discarded the table, and once reachable the compiler
folded the walk to a constant. `volatile` is what makes it real.

**Still open:** no single binary can run all 34 demo files, and that is a
property of the demo files — `flop.c`/`sp_flop.c` and
`comtest.c`/`comtest_strings.c` are mutually exclusive pairs defining the same
symbols, `MessageBufferAMP.c`'s `sbSEND_COMPLETED` is process-wide, and two
files ship no checker at all.

## Using it

Today this is consumed by the two cells in this repository rather than as a
library. From the umbrella:

```sh
# 25 demo files on an emulated Cortex-M3
cd rusty_rtos-capi/firmware/mps2-an385-qemu-capi && cargo run --release

# the same files on OS threads
cd rusty_rtos-capi/hosted/capi-host && cargo run --release

# one demo, or a comma-separated set, to bisect an interference
KAIROS_CAPI_ONLY=BlockQ,EventGroups cargo run --release

# the demos' own cadence: three checks, 10,000 ticks apart
KAIROS_CAPI_TICKS=30000 cargo run --release
```

The publishable crate itself gives you the ABI's rules — `ctypes` (the C's
widths, following the target the way `portmacro.h` does), the handle codec, and
`symbols`, which is the ABI as a data table plus the header writer.

## Performance

No timing rows: this package's claim is behavioural, and the numbers that
matter are the kernel's and the port's.

One measured configuration is worth recording, because it was mistaken for a
kernel defect first. `StreamBufferDemo` on the emulated M3 had 2 of ~290
trigger-test receives block six ticks against a five-tick request — exactly 2
whether the run was 30,000 or 60,000 ticks, with a sticky checker so two events
poisoned the rest. The host did it **zero** times with the same kernel, same
seam and same demo, so what differed was not code but how much CPU a tick gets:

| emulated CPU clock | over-blocks per 30,000 ticks |
|---|---:|
| 20 MHz | 2 |
| 40 MHz | 2 |
| **80 MHz** | **0** |

At 80 MHz the byte histogram is `[_, _, 58, 58, 58, 115, 0, 0, 0, 0]`,
**identical to the host's**. That identity is the evidence, not the pass. The
demo ships `configSTREAM_BUFFER_TRIGGER_LEVEL_TEST_MARGIN` to widen the
assertion; no FreeRTOS project sets it and neither do we — one `#define` would
have turned the run green in a minute and hidden the measurement instead of
making it.

## Portability

One seam, compiled into both cells, naming no chip and no host: every
platform-shaped thing is one of six names the cell supplies.

| cell | port | stacks | result |
|---|---|---|---|
| `firmware/mps2-an385-qemu-capi` | Cortex-M, QEMU `mps2-an385` | a static arena plus `init_stack` | 25/25 |
| `hosted/capi-host` | host, OS threads | the operating system's | 25/25 on Windows and Linux |

`BaseType_t` is 32 bits in one cell and 64 in the other, and neither the seam
nor a demo file needed a line changed.

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

## Part of Remade With Rust

This crate is part of **[Kairos](https://github.com/Remade-With-Rust/kairos)** —
FreeRTOS remade in memory-safe Rust, as independent packages that expose the API
a FreeRTOS developer already knows and prove every scheduling decision against
the C kernel's own trace. `rusty_rtos-capi` is the bridge that lets code written for FreeRTOS run on it unchanged.

**Where this sits for Mata.** Kairos is the real-time layer on the device
itself, and [`rusty_rtos_mqtt`](https://github.com/Remade-With-Rust/rusty_rtos_mqtt) is the way out of it.
Paired with the **MATA distributed cloud**, robotics and sensor data has two
routes — read it on the machine, or reach it through the cloud — with the same
memory-safe crates at both ends.

The family:
[`rusty_rtos_core`](https://crates.io/crates/rusty_rtos_core) (the shared vocabulary),
[`rusty_rtos_kernel`](https://crates.io/crates/rusty_rtos_kernel) (the scheduler),
[`rusty_rtos_port`](https://crates.io/crates/rusty_rtos_port) (the architecture seam),
[`rusty_rtos_heap`](https://crates.io/crates/rusty_rtos_heap) (the allocators),
[`rusty_rtos_json`](https://github.com/Remade-With-Rust/rusty_rtos_json) (coreJSON),
[`rusty_rtos_sntp`](https://github.com/Remade-With-Rust/rusty_rtos_sntp) (coreSNTP),
[`rusty_rtos_mqtt`](https://github.com/Remade-With-Rust/rusty_rtos_mqtt) (coreMQTT),
[`rusty_rtos_backoff`](https://github.com/Remade-With-Rust/rusty_rtos_backoff) (backoffAlgorithm),
[`rusty_rtos-capi`](https://github.com/Remade-With-Rust/rusty_rtos-capi) (the C ABI) and
[`rusty_rtos_demo`](https://github.com/Remade-With-Rust/rusty_rtos_demo) (the conformance corpus).
The last six are on GitHub and not yet on crates.io. Also check out
the rest of **[github.com/remade-with-rust](https://github.com/remade-with-rust)**.

## About Mata Network

<!-- ORG BOILERPLATE — keep identical across repos -->

**[Mata Network](https://www.mata.network/)** builds sovereign, self-hostable
privacy infrastructure — *"stop sacrificing your privacy for convenience"*:
wallet & identity, a password manager, a contact manager, and a browser
extension that stops your information leaking as you browse.

**Remade With Rust** is our open-source home for the permissively-licensed
building blocks that work depends on — including
[remade_ffmpeg_rs](https://github.com/Remade-With-Rust/remade_ffmpeg_rs) (the
FFmpeg alternative) and [FFAI](https://github.com/Remade-With-Rust/FFAI) (the
AI media toolkit).

→ **[www.mata.network](https://www.mata.network/)**

<!-- /ORG BOILERPLATE -->

## License

MIT OR Apache-2.0, at your option. FreeRTOS is MIT-licensed by Amazon.com,
Inc. or its affiliates; this crate remakes its API and behaviour from the
published sources and links no FreeRTOS code.

---

<!-- HARDENING-TABLE:BEGIN generated by use-protection-please — edit docs/plans/use-protection-please.md, not this block -->
## Hardening status

**Tier** critical-path · **Audited** 2026-09-16 (v0.1.0 release pass) · **v1.0.0 gates** 7/17 · [Full checklist](https://github.com/Remade-With-Rust/rusty_rtos-capi/blob/main/docs/plans/use-protection-please.md)

`██████░░░░░░░░░░░░░░` **31%** &nbsp;·&nbsp; 11 Completed · 0 Scheduled · 25 Incomplete · 19 N/A

| Phase | ✅ Completed | 🗓 Scheduled | ⬜ Incomplete | · N/A |
|---|--:|--:|--:|--:|
| 0 — Threat modeling | 0 | 0 | 2 | 0 |
| 1 — Toolchain | 2 | 0 | 2 | 0 |
| 2 — Supply chain | 5 | 0 | 3 | 0 |
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
| **Total** | **11** | **0** | **25** | **19** |

Gates waived for 0.x are listed with their reasons in the plan's "v0.1.0 release decision" section — an Incomplete gate not listed there is an omission, not a decision.

**Architect** — [Tim Almond](https://github.com/Ttimmahlax) — accountable for this unit's security design; rendered
<!-- HARDENING-TABLE:END -->
