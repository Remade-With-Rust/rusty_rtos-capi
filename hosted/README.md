# hosted/

Cells that run on the machine you are reading this on, rather than on a chip.

`firmware/` is for a part with a target triple and a linker script. These are
ordinary host binaries, and they are kept apart for the same reason
`firmware/` is: each is its own cargo project, excluded from the workspace,
because a cell fixes a port and a geometry and a workspace cannot hold two
answers to either.

| cell | what it claims |
|---|---|
| `capi-host` | the 21 unmodified C demo files, on `rusty_rtos_port-host`: one OS thread per task, real stacks |

## The two C ABI cells are one experiment on two ports

`capi-host` and `firmware/mps2-an385-qemu-capi` compile **the same
`seam/abi.rs` and the same `seam/demos.rs`**, byte for byte. They differ in
the port beneath them and in the six names the seam asks every cell to
supply:

| name | Cortex-M3 | host |
|---|---|---|
| `yield_now` | pend a `PendSV` | hand the run permit over |
| `without_interrupts` | `cpsid i` / `cpsie i` | take the port's critical lock |
| `note` | semihosting `hprintln!` | `eprintln!` with the thread's name |
| `die` | semihosting `debug::exit` | `std::process::exit` |
| `with_kernel` | the kernel behind a masked-interrupt borrow | the same, behind the same lock |
| `arm_task` | a static stack plus `init_stack` | an OS thread |

Two ports is not redundancy. `BaseType_t` is 32 bits in one and 64 in the
other, and that difference alone found two width bugs that no compiler could
see on either port alone: a C function pointer kept in an `AtomicU32`, and
demo entry points declared `u32` against the C's `UBaseType_t`.

## Running one

```sh
cargo run --release                       # all 21 demo files together
KAIROS_CAPI_ONLY=BlockQ cargo run --release   # one alone
```

The filter exists because demo files share a kernel, and sharing means
interfering: a task that never blocks starves every lower-priority task in
every OTHER demo. Individually-pass/together-fail is a different diagnosis
from a broken symbol, and without the filter the two are indistinguishable.

## Diagnostics

All gated at compile time, so they cost nothing in a normal run.

| | |
|---|---|
| `KAIROS_CAPI_PROBE=queueset` | replay `QueueSet.c`'s setup sequence one call at a time, printing each answer against the one the C requires |
| `KAIROS_CAPI_PROBE=states` | print every task's state and priority at each `eTaskGetState`, and log every scheduler decision with the thread that caused it — for diffing a failing port against a passing one at the same program point |
| `KAIROS_HOST_NO_PREEMPT=1` | decline preemption at runtime: the poison test, and what the platforms without a backend look like |
