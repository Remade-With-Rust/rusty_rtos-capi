# rusty_rtos-capi — package plan

**One sentence:** The FreeRTOS C ABI over the Kairos kernel: xTaskCreate, xQueueSend, xSemaphoreTake, xTimerCreate and the rest as extern C symbols with generated FreeRTOS.h-compatible headers, so a C program relinks and the unmodified C demo tasks pass.

Family plan: Kairos `docs/plans/rtos-mission.md` (umbrella repo) — its §2.1
names what this package remakes, wraps and never touches; its §6 carries the
phase this package's kill test belongs to. This file obeys that one.

Written 2026-09-09. Updated 2026-09-12. Status: **both halves of the kill test
pass.** 25 unmodified C demo files pass individually and together on QEMU
Cortex-M3, on Windows threads and on Linux pthreads, every verdict the demo
file's own checker. Under the demos' own 10,000-tick check cadence — a
strictly stronger question — all three are 21/21, and reproducibly so. See
`docs/LEDGER.md` for the runs and the thirteen defects they found.

---

## 1. What it is, what it is not

**Is:** the Rust remake of the FreeRTOS component named above, exposing the
names a FreeRTOS developer already knows, with the C original as the oracle.

**Is not:** a binding to the C code, a fork of it, or a place where a chip's
registers are touched (that is a port crate).

## 2. The laws this package encodes

1. The core is `no_std` (+ `alloc`), `forbid(unsafe)`, arch-agnostic.
2. Every parser that takes bytes from a wire, a store or a bus has a
   `tests/no_panic.rs` from the day it exists.
3. Every claim has a kill test or a ledger row; the README copies this plan
   and never upgrades it.
4. Feature ladder `std` ⊃ `alloc` ⊃ core-only; CI proves the two bare-metal
   rungs on four targets on every push.

## 3. The surface as built

**87 `extern "C"` symbols**, and the number is derived rather than chosen: the
unmodified demo files were compiled and `llvm-nm -u` was asked what they wanted.
That list IS the requirement, and it is far smaller than the API map suggests —
108 undefined symbols across 33 files, of which 11 are `__aeabi_*` compiler
helpers and 6 are board drivers.

| where | what |
|---|---|
| `crates/rusty_rtos-capi-core` | the RULES, `forbid(unsafe)`, 38 tests: the handle codec, the item codec, the C string reader, the copy positions, the retry protocol, and the ABI as a data table |
| `seam/abi.rs` | the symbols themselves — one file, compiled into every cell, naming no chip and no host |
| `seam/demos.rs` | the demo files' entry points, their checkers and their ISR halves |
| `firmware/mps2-an385-qemu-capi` | the Cortex-M3 cell |
| `hosted/capi-host` | the host cell, on OS threads |
| `tools/derive_symbols.py` | re-derives the symbol table from the seam; `--check` reports drift |

The core crate holds the rules and the cells hold the pointers. That split is
not tidiness: five hand-written copies of the handle encode lived in the seam,
and only ONE of them had the null-handle guard that `GenQTest.c:564` had already
found. A rule written five times is a rule fixed once.

### The generated header

`kairos_capi.h` comes from the symbol table at build time and is never checked
in — a generated file in the tree is a file somebody edits, and then nobody can
tell which is authoritative. Two things are checked about it by the toolchain
rather than by reading, and both are in `capi/header_gate.c`:

* it is compiled **in the same translation unit as the oracle's real FreeRTOS
  headers**, so a declaration of ours that disagrees with theirs is a compile
  error (it found four);
* it takes the **address of every declared symbol**, so a declaration with no
  definition is an undefined reference at link time (poison-tested).

## 3.1 What a consumer does

Compile against the oracle's headers as you already do, add `kairos_capi.h`,
and link against a cell. Nothing in the C changes — that is the whole claim,
and the thirty-three demo files are the evidence for it.

## 4. Roadmap

| Milestone | Adds | Driven by | Kill test |
|---|---|---|---|
| scaffold | the shape | K0 | a clean clone builds alone; CI green — **done 2026-09-09** |
| the M3 cell | the seam, 87 symbols, the demo table | K6 | 25 demo files pass together on QEMU `mps2-an385` — **done 2026-09-11, extended 2026-09-12** |
| the core crate | the rules, with tests they never had | K6 | 38 tests, including one per defect the C found — **done 2026-09-11** |
| the generated header | `symbols.rs`, the deriver, the gate | K6 | the gate compiles beside the oracle's headers and links every symbol — **done 2026-09-11** |
| the host cell | a second port under the same seam | K6 | the same 21 files pass together on OS threads — **done 2026-09-11** |
| sustained checking | the together-run clean at the demos' own 10,000-tick cadence | K6 | **done, both ports 21/21.** The last gap was the emulated cell's CPU budget, not the kernel: 20,000 cycles a tick could not carry 60 tasks plus every demo's ISR half, so `StreamBufferDemo`'s zero-margin trigger test saw 2 late receives in ~290. At 80,000 the histogram matches the host's exactly and the count is 0 |
| pthreads | the Unix half of the host port's preemption | K6 | the same files pass on Linux, sustained (21 on 2026-09-11, 25 after the 2026-09-12 expansion) — **done.** Unix cannot stop a thread from outside, so the target parks ITSELF in a `SIGUSR1` handler until `SIGUSR2`; `freeze` waits to see it parked, and the thaw signal is blocked inside the handler so it cannot be lost |
| the 34th file | `IntQueue.c`, which needs a board's `IntQueueTimer.h` | K6 | it is a demo-project file, not an ABI gap — **deliberately absent** |

## 4b. What "all 34 demo files" would actually take (2026-09-12)

The corpus is 34 files in `Demo/Common/Minimal`. Running them all is a
natural-sounding target and it is **not achievable in one binary**, for
reasons that are properties of the demo files rather than of this kernel.
Each was found by the compiler, the linker or a failing assertion, not by
reading.

### Why one binary cannot hold 34

| constraint | files | evidence |
|---|---|---|
| **duplicate symbols** | `flop.c` / `sp_flop.c` | both define `vStartMathTasks` and `xAreMathsTaskStillRunning`; they are the double- and single-precision ALTERNATIVES, not additions. `llvm-nm --defined-only` on both objects, identical |
| **duplicate symbols** | `comtest.c` / `comtest_strings.c` | both define `xAreComTestTasksStillRunning` |
| **a global macro** | `MessageBufferAMP.c` | `sbSEND_COMPLETED` is process-wide: with it in force every other stream-buffer demo routes there, and `vGenerateCoreBInterrupt` reaches a control buffer that exists only after the AMP demo starts. **Solved**: its own binary, `--features amp`, mirroring the oracle's `-DKAIROS_AMP=1` |
| **the timer command queue** | `flash_timer.c` vs `TimerDemo.c` | `prvTest1_CreateTimersWithoutSchedulerRunning` starts exactly `configTIMER_QUEUE_LENGTH` timers before the scheduler runs and asserts every start SUCCEEDS. `vStartLEDFlashTimers` starts three in the same window, so `TimerDemo.c:313` fails. **Raising the queue length does not help** — TimerDemo sizes its own test from that macro, so it always fills exactly the queue there is |
| **no checker at all** | `flash.c`, `flash_timer.c` | each exports its start function and nothing else. There is no `xAre...StillRunning`, so there is no verdict to take, and writing one would be our opinion wearing the demo's clothes |
| **a board header** | `IntQueue.c` | includes `IntQueueTimer.h`, which each demo *project* supplies. The only one of the 34 that does not compile |

So the honest ceiling is **31 files with a verdict of their own**, across at
least three binaries, plus two that can run but cannot be graded.

### Where it stands

| | count |
|---|---:|
| files with a checker, passing in the main binary | 25 |
| in its own binary (`--features amp`) | 1 |
| ran, **no checker of their own** — reported, never counted | 1 (`flash.c`) |
| files with a checker still not running | 5 |
| files that can never be graded | 2 |
| does not compile | 1 |

Getting here from 21 cost four defects, and none of them was in the ABI
shim alone: `pcTimerGetName` returned an empty string against an assertion
that compares it; the timer daemon slept on the delayed list instead of
waiting on its own queue, so a posted command could not preempt it (and the
kernel had no way to express the block time at all); `MESSAGE_LENGTH_BYTES`
disagreed with the C by four bytes on a 64-bit host; and the default run was
SHORTER than the 10,000-tick period every checker in the corpus is written
against, which produced a false failure the moment the load changed. The
`docs/LEDGER.md` entries carry the measurements.

### The three decisions, which are the owner's

**1. Co-routines — `crflash.c` and `crhook.c`.** Both compile to EMPTY
objects today: their whole body is behind `configUSE_CO_ROUTINES`, which
this project does not define. Running them needs a second scheduler —
`xCoRoutineCreate`, `vCoRoutineSchedule` from the idle hook,
`xCoRoutineRemoveFromEventList`, `vCoRoutineAddToDelayedList`,
`vCoRoutineResetState`, and `xQueueCRSend`/`xQueueCRReceive` with their
from-ISR halves. Nine symbols and a co-operative scheduler beside the
pre-emptive one.

*The case against:* FreeRTOS itself treats co-routines as legacy and its own
documentation discourages new use. No consumer has asked. It is the largest
single piece of work left in this package and it buys two demo files with
the lowest value in the corpus.

*The case for:* they are part of the C API a legacy codebase may already be
written against, and this package's whole thesis is that the switching cost
is what matters.

*Recommendation:* **not now.** Revisit only if a real consumer's code names
`xCoRoutineCreate`, which is a question the derived-surface method answers in
one command against their sources.

**2. Static allocation — `StaticAllocation.c`.** Also an empty object today
(`configSUPPORT_STATIC_ALLOCATION` is 0). It needs `xTaskCreateStatic`,
`xQueueCreateStatic`, `xTimerCreateStatic`, `xEventGroupCreateStatic` and
the application's `vApplicationGetIdleTaskMemory` /
`vApplicationGetTimerTaskMemory`.

*The real question is not the symbols.* Kairos places every object in an
arena declared at compile time, so there is nothing for the C side to place
an object INTO — a caller handing us a `StaticTask_t` is offering storage the
kernel has no way to use. Supporting it means either accepting
caller-supplied storage in the kernel (a design change with a safety story
to write) or accepting the buffer and ignoring it (which compiles, passes
the demo, and is a lie).

*Recommendation:* **a design decision, not a task.** It is the one item here
that could change the kernel's memory model, and K4 is where that
conversation belongs. Ignoring the caller's buffer is the only cheap option
and it should be refused.

**3. `IntQueue.c`.** Needs `IntQueueTimer.h`, a file every demo *project*
supplies for its own board, plus genuinely nested interrupts at two
priorities. The QEMU cell could supply the header; the nesting is a port
question for `rusty_rtos_port-cortex-m`.

*Recommendation:* **worth doing, and it is a PORT item rather than an ABI
one.** It is the only one of the three that tests something this kernel
claims and cannot yet demonstrate — from-ISR correctness under nesting. It
belongs with K8's MPU/nesting work on the M33, not here.

## 5. Deliberately absent

* **A FreeRTOS.h replacement.** `kairos_capi.h` declares what this ABI exports
  and typedefs the handles; it does not try to be the kernel's header. The K6
  cells compile the demos against the ORACLE's headers on purpose, because
  compiling them against ours would prove only that we agree with ourselves.
* **`IntQueue.c`**, the one demo file of 34 that does not compile. It includes
  a board-specific `IntQueueTimer.h` that each demo *project* supplies. Not an
  ABI gap.
* **Static allocation.** `configSUPPORT_STATIC_ALLOCATION` is 0 in both cells:
  Kairos allocates from arenas declared at compile time, so there is nothing
  for the C side to place objects in. `StaticAllocation.c` is K4's cell.
* **A queue item wider than eight bytes.** A Kairos queue slot holds a `u64`;
  beyond that the two are not the same thing, and `xQueueGenericCreate` refuses
  rather than truncating. A queue that silently dropped four bytes of every
  item would produce a demo that "runs" and is wrong.

## 6. Risks

| Risk | Mitigation |
|---|---|
| The seam is one file compiled into every cell, so a cell can drift by supplying its six names differently | the names are a documented table in the file's own header, and both cells are run in CI; a third cell's first job is to run the same 21 files |
| The generated header and the seam could disagree | they cannot: `derive_symbols.py` produces the table from the seam and `--check` reports drift, and the header gate compiles the result beside the oracle's own declarations |
| Width assumptions that are invisible on one port | two ports with different `BaseType_t` widths is the mitigation, and it has already caught two (a pointer in an `AtomicU32`, a `u32` priority argument). A third port that is neither 32- nor 64-bit would need a third look |
| The host port's preemption is Windows-only | `PREEMPTIVE` reports it and the cell warns; the failure mode without it is a starved demo, which reads as somebody else's bug, so it is said out loud |
| Semantic conformance is open-ended: the demos reach edge cases the corpus's 19 scenarios do not | that is the point of running them, and every defect so far has been named by a `configASSERT` rather than by us. `configASSERT` prints and exits rather than being compiled out |

## 7. Decision log

| Date | Decision |
|---|---|
| 2026-09-09 | Stamped from the Kairos template; obeys the family plan. |
| 2026-09-11 | **The surface is derived, not designed.** Compile the unmodified demo files and ask `llvm-nm -u`. 108 symbols across 33 files, not the API map's 341. Nobody gets a vote, and `tools/derive_symbols.py` keeps it that way. |
| 2026-09-11 | **`configASSERT` is LOUD, not compiled out.** A demo tripping one is the C telling us the ABI lied to it, and that is the most valuable thing a cell can produce. It now prints the task-state table with it, because most of these assertions are about scheduling and the line number alone says only which check disagreed. |
| 2026-09-11 | **One seam, compiled twice, not two seams kept in step.** `extern "C"` functions cannot be generic and the kernel's geometry is nine const parameters, so a cell must compile the seam rather than link it. The seam therefore names no chip and no host: six names, supplied per cell. |
| 2026-09-11 | **The C's scalar types follow the PORT, not a constant.** `portBASE_TYPE` is `long` on ARM_CM3 and `long long` under `_WIN64`; `ctypes` asks the pointer width, which is the same question `portmacro.h` asks, and the generated header emits the same conditional. Two cells that disagree about it is a feature — it is what caught two width bugs no compiler would have. |
| 2026-09-11 | **The derived surface depends on the port header.** `ARM_CM3` expands `portYIELD()` into an SCB write; `MSVC-MingW` expands it into `vPortGenerateSimulatedInterrupt`. The derivation is allowed to notice that, which is why the surface is 89 and not 88. |
