# rusty_rtos-capi — the ledger

Every number this package claims, with the run that produced it. A row
without a method is not a number. Counters before clocks; an external oracle
before a self-metric; the method line names the machine, the pinning, the arm
order and the null-arm floor for anything timed.

## The build fact (2026-09-09)

| gate | result |
|---|---|
| `cargo check --workspace` on the host | passes at scaffold |
| `cargo check -p rusty_rtos-capi-core --no-default-features` and `--features alloc` on `thumbv7em-none-eabihf`, `thumbv8m.main-none-eabihf`, `riscv32imac-unknown-none-elf`, `riscv32imafc-unknown-none-elf` | passes at scaffold (`kairos check`) |
| `cargo clippy --workspace --all-targets -- -D warnings` under the workspace lint policy | clean at scaffold |
| `cargo deny check` | see the hardening plan's H-08 row |

The scaffold row above is superseded by everything below it: the package has
run on a chip and on a host, and the numbers are the demos' own.

## K6 — 26 demo files, and the four defects the five new ones found (2026-09-12)

Five more files from `Demo/Common/Minimal` now run: `TimerDemo`, `flop`,
`MessageBufferDemo`, `comtest` and `flash` — and `MessageBufferAMP` in a
binary of its own. Twenty-six files, twenty-five of them reporting their own
verdict.

| | each alone | together | together, sustained |
|---|---:|---|---|
| QEMU Cortex-M3 | 25/25 | **25/25** | **25/25** |
| host, Windows threads | 25/25 | **25/25** | **25/25**, 3 runs of 3 |
| host, Linux pthreads | 25/25 | **25/25** | **25/25**, 3 runs of 3 |
| `--features amp`, its own binary | **1/1** | — | — |
| `flash.c` | ran; **no checker of its own**, never counted | | |

**Which files were added was decided by the linker, not by preference.** The
undefined symbols of all 33 compiled files were diffed against the 89 the
seam exports, per file. Nine of the twelve that were not running needed
NOTHING from the kernel — only C library functions and a demo project's
board drivers. That is what made this worth doing now: no new ABI surface.

### The four defects

1. **`pcTimerGetName` returned an empty string.** The kernel keeps a
   timer's name in a fixed-width `Name`, which is a Rust string with a
   length, so there was no NUL-terminated buffer to hand back that outlived
   the call — and returning `""` was the honest placeholder. It passed every
   demo in the corpus and failed on the FIRST LINE of the one that looks:
   `TimerDemo.c:304` is `configASSERT( strcmp( pcTimerGetName( t ), "FR
   Timer" ) == 0 )`. Fixed in the SHIM rather than the kernel: NUL
   termination is an ABI representation, and FreeRTOS's own answer is a
   `char pcTimerName[]` inside the timer object, which is what
   `TIMER_NAMES` now is.

2. **The timer daemon slept beside its queue instead of waiting on it — and
   the kernel could not express anything else.** `TimerDemo.c:469` stops an
   auto-reload timer and asserts on the next line that it is inactive; its
   own comment says why it may, "this will appear to happen immediately to
   this task because this task is running at a priority below the timer
   service task". That only holds if posting the command PREEMPTS.

   The kernel gets that right — `queue_send_generic` calls `port_yield` when
   a send removes a waiter from the queue's receive list — but our daemon was
   never on that list. `process_one_timer_command` hardcoded a **zero** block
   time, so the daemon polled, found the queue empty and `delay(1)`ed onto the
   DELAYED list. No waiter, nothing to preempt for, and being higher priority
   bought it nothing.

   Two tests now pin both halves in the kernel itself, with a port that
   counts switch requests: a send to a parked higher-priority receiver asks
   for one, and a send with no waiter correctly asks for nothing. The second
   is the one worth having — without it, a reader who finds the send not
   yielding concludes the send is broken.

   `process_one_timer_command` now takes the block time.
   `prvProcessTimerOrBlockTask`'s own number — the time to the next expiry —
   is what both cells pass. The corpus runner and the no-panic gate pass 0,
   so their behaviour is unchanged and all 19 scenarios stayed
   byte-identical.

   **Measured, at the failure:** `daemon: worked=17 parked=14 polled=0 fell
   back=10`. `polled=0` refuted "it never blocks"; `fell back=10` was the
   answer — the timed-out arm still slept.

3. **`MESSAGE_LENGTH_BYTES` disagreed with the C by four bytes on a 64-bit
   host.** `configMESSAGE_BUFFER_LENGTH_TYPE` defaults to `size_t`, so the C
   spends eight bytes per message on a host and four on the chip. The Rust
   `Config` trait defaults it to 4 and neither cell said otherwise, so
   `xMessageBufferSpaceAvailable` came back four short of the demo's own
   arithmetic and `MessageBufferDemo.c:262` failed on the first message.

   **The trait's own doc had predicted it** — "a configuration that claims to
   match a given kernel has to say which" — and nobody said. It is the same
   class as `configTIMER_QUEUE_LENGTH 10` against `2`: compiles cleanly on
   both sides, wrong on one port only. It is now DERIVED from the target in
   `rusty_rtos-capi-core::ctypes`, where the C's widths already live, so
   there is nothing left to get wrong.

4. **The default run was shorter than the period the checkers are written
   against.** `CHECK_PERIOD_TICKS` is 10,000 because `main_full.c`'s check
   task is; the default run was 3,000. So the default asked "has your counter
   moved" before any demo had been given a period to move it in — the weak
   form, which this ledger already said, but worse than weak: it produces
   FALSE FAILURES. One arrived the moment `flop.c` joined the set. Its four
   never-blocking tasks sit at the idle priority, which is where
   `TaskNotifyArray`'s task also sits, so that demo got a fifth of the CPU it
   used to, needed more than 3,000 ticks for one test pass, and reported FAIL
   at 3,000 while passing three times over at 30,000. The default is now one
   full check period.

### And one defect in the harness's own new code, which is the instructive one

The serial loopback returned `Wait::Blocked` to the C **as failure**. In this
kernel `Blocked` means "call again when this task next runs", and every queue
function in `seam/abi.rs` loops on it; my `xSerialGetChar` did not.
`comtest.c`'s receive loop advances its expected byte whether or not a byte
arrived, so one spurious failure per character desynchronised the sequence
permanently, and its checker said NO for the rest of the run.

The bytes were in perfect order the whole time. A one-line trace said so —
`WIRE rx FAILED` before every single `WIRE rx 65` — and that is the whole
diagnosis: **a driver that gets the retry protocol wrong looks exactly like a
kernel that loses data.** Worth recording because the next person to write a
consumer of this ABI will reach for the same shortcut. After the fix: 3,024
bytes transmitted, 3,024 received, in order, both ports.

### What could not be added, and why it is about the demo files

| constraint | files |
|---|---|
| duplicate symbols — they are ALTERNATIVES | `flop.c` / `sp_flop.c` (`vStartMathTasks`), `comtest.c` / `comtest_strings.c` (`xAreComTestTasksStillRunning`) |
| a process-wide macro | `MessageBufferAMP.c`'s `sbSEND_COMPLETED` — now in its own binary, as the oracle does it |
| the timer command queue | `flash_timer.c` starts timers before the scheduler; `TimerDemo.c` fills that queue deliberately and asserts every start succeeds, so it fails. **Raising the queue length cannot help** — TimerDemo sizes its test from that macro |
| **no checker at all** | `flash.c`, `flash_timer.c`: a start function and nothing else |
| a board header | `IntQueue.c` |

So **no single binary can ever run all 34**, and two of the 34 can never
report a verdict. `docs/plans/rusty_rtos-capi.md` §4b carries the arithmetic
and puts co-routines, static allocation and `IntQueue` as three separate
decisions.

### Where `sbSEND_COMPLETED` lands when the kernel is Rust

Worth its own note. In the C the macro is expanded by `stream_buffer.c`,
which we do not compile — so there is no macro to override. The Rust kernel
is the thing that decides what a completed send does, and it already had the
seam: `TickHook::send_completed` answering `true` means "handled, do not
notify", which is exactly the macro's contract. The AMP cell implements that
one method and the unmodified demo works.

### The board drivers are a demo PROJECT's files, not the ABI's

`seam/board.rs` supplies `vParTestSetLED`, `vParTestToggleLED`,
`vParTestInitialise` and four serial functions. None is a FreeRTOS API, so
none is in `symbols.rs` or the generated header — claiming otherwise would
be a lie about what FreeRTOS exports. That left them audited by nothing,
which `seam/board_gate.c` fixes the same way the header gate does: include
the oracle's own `partest.h` and `serial.h`, take the address of each, keep
the table `volatile` so the linker cannot discard the check.

The serial port is a **loopback** because that is what `comtest.c` asks for
in words: "a loopback connector should be used so that everything that is
transmitted is received". The wire is a kernel queue, so the driver is also
a consumer of the thing under test.

## K6 — 21 of 21 unmodified C demo files, two ports, three platforms (2026-09-11)

The kill test reads "the unmodified C standard demo tasks link against
`rusty_rtos-capi` and pass **on Posix, then on QEMU M3**". Both halves now
run, in the other order, and the reason for the inversion is recorded below.

| cell | port | stacks | each alone | all together |
|---|---|---|---|---|
| `firmware/mps2-an385-qemu-capi` | `rusty_rtos_port-cortex-m`, QEMU `mps2-an385` | a static arena plus `init_stack` | **21/21** | **21/21**, reproducible |
| `hosted/capi-host`, Windows | `rusty_rtos_port-host`, Win32 threads | the operating system's | **21/21** | **21/21**, 3 runs of 3 |
| `hosted/capi-host`, Linux | the same crate, pthreads | the operating system's | **21/21** | **21/21**, 3 runs of 3 |

Method: `cargo run --release` in each cell; 3,000 ticks at 1 kHz; every
verdict is the demo file's OWN checker, never ours.
`KAIROS_CAPI_ONLY=<name>` runs one file alone, which is how an
individually-pass/together-fail is told from a broken symbol. The together
runs: 60 tasks, 25 queues, 3,000 of 3,000 ticks on both; QEMU's deepest
stack 122 of 512 words; the host 17,929 switches.

### How hard the question is, and it was not hard enough

The demos ship with a check task that asks every checker **periodically**
and latches any failure. `Demo/Posix_GCC/main_full.c`:

```c
const TickType_t xCycleFrequency = pdMS_TO_TICKS( 10000UL );
```

Asking once at the end of a 3,000-tick run is the WEAKEST form of that
question. It asks "did this counter ever move", not "is it still moving",
and `xAreTaskNotificationArrayTasksStillRunning` only exercises its
course-cycle branch every THIRD call, so one call never runs that half at
all.

Both cells can now sample at the demos' own cadence
(`KAIROS_CAPI_TICKS=30000` gives three checks 10,000 ticks apart), and a
demo passes only if EVERY sample passed. That is strictly stronger, and it
found **seven** more defects — including three the weak form had been hiding
on BOTH ports, and one that turned the host's together-run from a coin
flip into 3 clean runs of 3. It is **not yet clean**: see "What sustained
checking still says" below.

Saying it plainly: the row above is 21/21 at a check strength the demos
would call generous.

Nothing about the C is patched, wrapped or regenerated. The demo files come
straight out of the pinned `oracle/` checkout and are compiled against the
oracle's own `FreeRTOS.h`, `task.h` and `queue.h`. The only file either cell
hands the C side is a `FreeRTOSConfig.h`, which every FreeRTOS application
supplies.

### One seam, two ports, six names

`seam/abi.rs` is ONE file compiled into both cells, not two files kept in
step. It names no chip and no host: everything platform-shaped is one of six
names the cell supplies — `yield_now`, `without_interrupts`, `note`, `die`,
`with_kernel`, `arm_task` (plus `where_it_runs`, for diagnostics).

That the list is that short is the finding. A C ABI over a stackless kernel
reads as though it must be full of architecture, and it is not.

`BaseType_t` is 32 bits in one cell and 64 in the other, and neither the
seam nor a demo file needed a line changed for it — both are written in
terms of the typedef rather than a width.

### Why the order inverted, and what closed it

The Kairos kernel is stackless: a blocking call answers `Wait::Blocked`,
meaning "call again when this task next runs". A C task cannot work that way
— `vTaskDelay` must return *later*, with its locals intact — so it needs a
real stack, and there was no host port that had one: the three real ports
were bare-metal and the sim port is stackless.

`rusty_rtos_port-host` is what closed it: one OS thread per task, a single
run permit, and a tick thread that can freeze whichever thread holds it.
The stacks are the operating system's. Its own kill test is
`rusty_rtos_port/firmware/host-kernel`.

### Defects closed, with the instrument that found each

1. **A port's critical section ENABLED interrupts instead of RESTORING
   them.** `CortexMPort::exit_critical` ended its outermost section with an
   unconditional `cpsie i`. `xTaskCreate` masks interrupts around "create
   the task" plus "give it a stack" — a task that is schedulable without a
   stack is one `PendSV` can pick and run at `SP = 0` — and the kernel's own
   `create_task` takes a critical section internally whose exit unmasked
   interrupts inside that supposedly-atomic pair. `StreamBufferDemo`'s echo
   server creates its client at a HIGHER priority than its own, which is
   the case that makes the new task the obvious next choice, and the run
   faulted to `pc = 0` about 360 ticks later, **in a different task, on a
   stack it did not own**. The tell was in the port's own file:
   `set_interrupt_mask_from_isr`/`clear_interrupt_mask_from_isr` had always
   saved and restored; the other pair had not.
2. **A queue SET had no item size, and its items were not translated.**
   `xQueueCreateSet` never registered one, so `xQueuePeek` on a set copied
   ZERO bytes and returned `pdPASS`, leaving the caller's handle at whatever
   it already held. And a set's items are member HANDLES, which cross the
   seam as `to_raw() + 1`: `xQueueSelectFromSet` translated them because it
   returns a handle directly, while peek and receive copied bytes and had no
   way to know what the bytes meant.
3. **The deferred `FromISR` work was routed nowhere.**
   `xEventGroupSetBitsFromISR` defers to the daemon task, which asks the
   application's `TickHook::pended` what to do with it. Both cells used
   `NoTickHook`, whose `pended` is a no-op, so the deferral was accepted,
   counted as `pdPASS`, and dropped. `EventGroupsDemo` set bits from the
   tick hook ten times and read `0x00` back every time.
4. **A 32-bit assumption in a table of pointers.** `CEntry` kept a C task's
   function pointer in an `AtomicU32`, which is correct on a Cortex-M3 and
   truncates every pointer on a 64-bit host. The first task to start jumped
   into nowhere and the process died with an access violation before a line
   of C ran.
5. **A 32-bit assumption in the demo declarations.**
   `vStartGenericQueueTasks(uxPriority: u32)` against the C's
   `UBaseType_t` — same width on ARM, not on the host, where the top half of
   the argument register was undefined.
6. **The host ran under the wrong identity between kernel calls.** On a
   chip the running task is whatever `PendSV` last restored, so the CPU
   FOLLOWS `Kernel::current` continuously. On a host the running task is an
   OS thread and nothing made it follow: a kernel call that moved `current`
   left the thread executing as somebody else until the next explicit
   yield, and every API that takes `NULL` to mean "the calling task" then
   named the wrong one. `GenQTest` caught it — `vTaskPrioritySet( NULL,
   genqMUTEX_LOW_PRIORITY )` executed by `MuLow`'s thread while the kernel
   believed `MuHigh` was running, so the HIGH priority task was set to low,
   and the assertion that noticed was three hundred lines away and about
   something else.

Two more were found by the port's own reasoning and fixed before they cost
a session: a task's voluntary handover could interleave with the tick
thread's, and `pend_switch` took a switch where `PendSV` would only have
PENDED one — re-entering the kernel from inside a kernel call.

### Five more, found only by asking the question properly

Every one of these was invisible to a single end-of-run check, and four of
the five were on **both ports** — so none of them is a host artefact.

7. **A self-deleted task was never reclaimed.** `prvCheckTasksWaitingTermination`
   is the idle task's other job in the C and is not optional: a task that
   deletes ITSELF cannot free its own slot, so the kernel defers and the
   idle task collects. Both cells' idle tasks only yielded, so the slots
   accumulated. `death.c`'s checker asserts the live task count stays
   within three of where it started; it took a 12,000-tick run to drift
   past that, and it failed **identically on both ports**, which is what
   said it was not a port bug.
8. **A software timer's callback was routed nowhere.** `TickHook::timer` is
   the daemon asking the application what a timer callback means — the same
   seam `pended` is, and a no-op for the same reason. And the table holding
   the `TimerCallbackFunction_t` was an `AtomicU32`, the same truncation
   that killed `CEntry` on a 64-bit host, sitting behind a comment of mine
   claiming it was "a small number, not a pointer". It was not.
9. **The timer daemon was half a daemon.** `prvTimerTask` has two halves —
   `prvProcessTimerOrBlockTask` for expiries and `prvProcessReceivedCommands`
   for commands — and both cells had only the second. So `xTimerStart`
   worked, the kernel put the timer on its list, and nothing ever took it
   off: `made=1 started=1 expiries=0`. `TaskNotify.c` starts a one-shot
   timer and blocks on `xTaskNotifyWait( ..., portMAX_DELAY )` for its
   callback, so its task blocked at tick 200 and was still blocked 29,800
   ticks later.
10. **`vTaskSuspend` did not clear a pending notification wait.** A KERNEL
    finding, and a precise one. `tasks.c`:

    ```c
    if( pxTCB->ucNotifyState[ x ] == taskWAITING_NOTIFICATION )
    {
        /* The task was blocked to wait for a notification, but is now
         * suspended, so no notification was received. */
        pxTCB->ucNotifyState[ x ] = taskNOT_WAITING_NOTIFICATION;
    }
    ```

    Not tidying: `eTaskGetState` calls a task on the suspended list BLOCKED
    rather than SUSPENDED if any notification slot is still waiting, so
    leaving the flag set makes a suspended task report as blocked for ever.
    `TaskNotify.c:498` asserts exactly that, inside a timer callback that
    suspends a task which is waiting for a notification.
11. **The two halves of one configuration disagreed.**
    `FreeRTOSConfig.h` said `configTIMER_QUEUE_LENGTH 10`; the Rust
    `CapiConfig` said `2`. That queue is where
    `xEventGroupSetBitsFromISR` and `xTimerPendFunctionCall` post their
    deferred work, so a burst was dropped. The config file's own header
    warns about exactly this — "the two halves have to AGREE, and where
    they must, it is written down" — and this is the class of bug it
    warns about: it compiles cleanly on both sides.
12. **An ISR half's wake-up was dropped on the floor.** The tick handler
    asked `increment_tick` whether to switch BEFORE running the demos' ISR
    halves, and `xNotifyTaskFromISR` passes NULL for
    `xHigherPriorityTaskWoken` as the C allows — so a task woken by an ISR
    half stayed ready and unscheduled until something else happened to
    yield. The seam now remembers it and the cells OR it into the switch
    decision, which is where `portYIELD_FROM_ISR` would be.

### One deliberate deviation from `main_full.c`, and it is the only one

`TaskNotifyArray` is commented out three times in upstream's authoritative
demo — its start, its check AND its ISR half — so upstream does not run it
at all. We do. Copying upstream's commented-out ISR line while
uncommenting its start line is not fidelity, it is half a configuration: the
demo blocks forever waiting for a notification nothing will send. So the
cells call `xNotifyArrayTaskFromISR()`, and the demo passes sustained
checking (`isr calls=54 delivered=54`, `waits=3999`, `takes=108`).

### What sustained checking says, now that it is clean

| 3 checks, 10,000 ticks apart | each alone | all 21 together |
|---|---|---|
| QEMU M3 | 21/21 | **21 / 21** |
| host | 21/21 | **21 / 21** |

It did not start there. Every failure it found read `ok NO NO`: the demo
passed the first check and never recovered. That pattern was worth more
than the count. A counter that merely failed to advance in one window
recovers at the next check and reads `ok NO ok`; one that never recovers
has STOPPED, and a demo whose checker latches a sticky error flag can do
nothing else. So they were never slow demos — they were stopped ones, and
that read the diagnosis before any instrument was built.

The two causes it exposed were not the same kind of thing: one was a defect
in our port, the other was the emulator running out of CPU. Telling those
apart is the whole of the work below.

### Where that led: a port that did not say it commits the switch

**Fixed.** `Port::COMMITS_SWITCH` tells the kernel whether the port takes
the switch itself. Every bare-metal port here declares it `true`;
`HostPort` did not, so it defaulted to `false` — and the kernel then
treated itself as STACKLESS, moving `current` inside `port_yield` by
calling `switch_context` directly. The port then switched AGAIN on the way
out.

**Two selections per yield.** The round robin advanced twice, and on a
ready list of two that is the same task every time, for ever.

The trait's own doc names the other half of the cost, and it is the same
sentence that describes a bug I had already worked around: "Between the
two, code runs as a task the kernel no longer thinks is current — and a
blocking call made in that gap parks the wrong task." That is exactly the
identity divergence `GenQTest` found earlier in this session, which I had
patched with a `reconcile()` function in the host cell. With the port
declaring itself properly there is nothing to reconcile, and `reconcile()`
was deleted rather than kept beside its own fix.

| | before | after |
|---|---|---|
| `firmware/host-kernel`, two never-yielding tasks at one priority | 13,489,456 laps against **0** | 6,077,079 against 6,065,221 |
| host C ABI cell, 21 demos, sustained checking | 19/21 | **21/21** |

### How it was found, and the three refutations on the way

The route matters more than the fix, because four plausible causes were
measured and killed first:

* **Arena exhaustion.** Refuted: tasks 114/56, queues 53/35, buffers
  13,372/13,365, heap free constant at 7,584 across the whole run.
* **A global stall.** Refuted: 62k / 64k / 63k switches per 10,000-tick
  period. The system was never slow; individual demos stopped.
* **`integer.c` hogging the CPU** — the one demo that never blocks and
  never yields. Refuted: removing it changed nothing.
* **`xTaskResumeAll` answering `pdTRUE` too often**, which `dynamic.c`
  latches an error on. Refuted by measuring both ports: 2,445 of
  20,840,685 on the host against 2,351 of 8,715,113 on QEMU — nearly the
  same absolute count, and both pass alone.

What did find it was a per-task turn count. `QProdB2` (BlockQ, priority 0)
on 107,299 turns beside `SetB` (EventGroups, priority 0) on **124** is not
a scheduling policy, it is a bug — the two are the same priority.

Then the smallest possible cell, then a probe in the list, then in
`switch_context`. Two tests were WITHDRAWN along the way because they
measured their own call pattern rather than the scheduler: `Kernel::delay`
yields as part of its contract, so a loop that calls `delay` and then
`switch_context` makes two selections and can only sample after the
second. Both are recorded as a note in `system.rs` rather than as
assertions.

### The last one was not ours, and proving that took a histogram

The failing demos' tasks were not blocked. They were **starved by an
equal-priority peer**, which fixed-priority scheduling does not permit: it
says what happens between different priorities and nothing at all about two
READY tasks of the same one. That gap is what `configUSE_TIME_SLICING`
fills, and the C fills it twice over — `xTaskIncrementTick` asks for a
switch whenever more than one task is ready at the running priority, and
`taskSELECT_HIGHEST_PRIORITY_TASK` then takes the NEXT entry rather than
the head.

`StreamBufferDemo` on QEMU M3 was the last demo still failing, and the
temptation with a failure this small is to call it environmental and move
on. That is a claim, and it needed evidence. So it was characterised to the
byte and the tick first:

* the demo's `prvInterruptTriggerLevelTest` blocks a receive for 5 ticks
  while the tick hook adds one byte per tick, and asserts on the exact
  count with **zero** margin;
* of ~290 such receives in a 30,000-tick run, **two** came back with 6
  bytes having blocked 6 ticks — the byte and tick histograms were
  identical, `[_, _, 58, 58, 55, 115, 2, 0, 0, 0]`, so those two really
  over-blocked by one tick rather than being overtaken after waking;
* the host did it **zero** times, with a textbook histogram
  `[_, _, 58, 58, 58, 115, 0, 0, 0, 0]`: levels 2/3/4 return their level,
  levels 5 and 6 both return 5;
* the count was **exactly two whether the run was 30,000 ticks or
  60,000**, beginning at ticks 5,708 and 22,840 — so neither a rate nor a
  startup transient, and a prediction that it tracked the monitor's own
  sampling was tested and refuted (60,000 ticks predicts five, gives two);
* the checker's `xErrorStatus` is **sticky**, so two events poison every
  later check — which is why the pattern read `ok NO NO`.

Same kernel, same seam, same demo file, same C: one port over-blocks twice
and the other never does. What differs between them is not code, it is how
much CPU a tick gets. So the hypothesis was the emulated part running out
of it — and the experiment is the one measurement discipline asks for:
change that single quantity and see whether the effect moves with it.

| cycles per 1 kHz tick | `configCPU_CLOCK_HZ` | over-blocks, 21 demos, 30,000 ticks |
|---:|---:|---:|
| 20,000 | 20 MHz | 2 |
| 40,000 | 40 MHz | 2 |
| **80,000** | **80 MHz** | **0** |

Sixty tasks plus a tick hook that runs every demo's ISR half EVERY tick,
sharing 20,000 cycles, is about 330 cycles per task per tick. At 80,000 the
histogram becomes `[_, _, 58, 58, 58, 115, 0, 0, 0, 0]` — **identical to
the host's**, which has real threads and no budget at all. That identity is
the evidence rather than the pass: the same code given enough CPU produces
the other port's numbers exactly, so what was being measured was the
emulator's throughput and not this kernel.

**The fix is a number in a config, and it is written down as a
measurement.** `configCPU_CLOCK_HZ` and the SysTick reload in `main.rs` are
two ends of one quantity in two languages with nothing checking them
against each other, so both carry the table above in a comment.
`configTICK_RATE_HZ` is untouched at 1000 — every demo counts in TICKS, so
no demo's timing changed; only how much work fits between two of them. The
cost is wall-clock: QEMU now runs 4x the instructions per tick, and a
sustained run takes correspondingly longer. Paid knowingly.

**What was NOT done, deliberately.** The demo's own source documents this
outcome — "an interrupt added another byte to the stream buffer before this
task was able to run" — and ships
`configSTREAM_BUFFER_TRIGGER_LEVEL_TEST_MARGIN` to widen the assertion. One
`#define` would have turned the run green in a minute. No FreeRTOS demo
project sets that margin, and neither do we: it would have hidden the
measurement instead of making it, and a knob that papers over a difference
answers a weaker question than the one this cell exists to ask.

`ready_cursor`, `ready_len`, `ready_items`, `grants_to`, `tick_stats` and
the stream-buffer histograms were added for these two hunts and stayed. "A
ready task is never chosen" has exactly two causes — it is not in the list
the scheduler looks at, or the cursor is not moving — and nothing else can
tell them apart. The histograms are why the last one could be ATTRIBUTED to
the emulator rather than merely blamed on it.

### A dead hypothesis, recorded so nobody re-runs it

`dynamic.c` latches an error when a `xTaskResumeAll()` it expects to be
quiet answers `pdTRUE`, and it is the host's most frequent together-run
failure — so the obvious theory was that a host tick thread makes that
happen more often. Measured, running `dynamic` alone on both ports:

| | `xTaskResumeAll` calls | answered `pdTRUE` |
|---|---:|---:|
| host | 20,840,685 | 2,445 |
| QEMU M3 | 8,715,113 | 2,351 |

Nearly the same absolute count, and **both ports pass alone**. It is not
the discriminator.

### And a defect the diagnosis itself had

The first three probes for the `pc = 0` fault came back clean, including a
spin guard on all twelve retry loops that fired on none of them. The reason
was that there was **no `HardFault` handler**: `cortex-m-rt`'s fallback is
an infinite loop inside an exception that outranks `SysTick`, so a faulting
C task presented as the whole system stopping, silently — indistinguishable
from a deadlock or a starved monitor. Eight lines of handler turned it into
`pc=0x0 cfsr=0x00020000` (INVSTATE) and a slot table naming a task whose
stack pointer was inside somebody else's arena.

Six permanent diagnostics came out of these hunts and stayed: the fault
handler, a stack-paint high-water report, a spin guard on every retry loop,
an invariant check in `pick_next` that refuses a saved stack pointer outside
its own task's stack, a task-state table printed by `configASSERT`, and the
host cell's identity check. The `pick_next` guard's first version
**exempted zero**, which is the worst value: `PendSV` skips its restore when
the incoming slot reads zero, so the outgoing task simply keeps running with
`CURRENT_SP_SLOT` already pointing at somebody else's slot.

### The method that did the work

The failures were all of the same shape — the C gives one `configASSERT`
for a dozen checks, so it can only say "something here is wrong". Three
instruments turned that into line numbers:

* **Replaying the C's sequence through the seam.** `QueueSet.c:1138` covers
  a dozen checks; walking `prvSetupTest`'s calls one at a time and printing
  each answer against the one the C requires named the failing call in one
  run (`KAIROS_CAPI_PROBE=queueset`).
* **Running the failing port against the passing one at the same program
  point.** `KAIROS_CAPI_PROBE=states` prints every task's state and
  priority at each `eTaskGetState`; the two cells' sequences agreed
  exactly at call 0 and diverged at call 1, which is where defect 6 lives.
* **An invariant that can be checked rather than argued.** "The kernel's
  current task is the thread that is running" is true by construction on a
  chip and has to be made true on a host. Asserting it directly named
  defect 6 at the first call that broke it, rather than at the assertion
  three hundred lines later.

### The header is generated, and the compiler audits it

`crates/rusty_rtos-capi-core/src/symbols.rs` holds the ABI as data: **89
symbols** with their C declarations. `tools/derive_symbols.py` re-derives
that table from `seam/abi.rs`, so the two cannot drift; `--check` reports
drift without writing.

The surface grew by exactly one for the host, and for a reason worth
keeping: `ARM_CM3`'s `portmacro.h` expands `portYIELD()` into a write to
the SCB, while `MSVC-MingW`'s expands it into
`vPortGenerateSimulatedInterrupt( portINTERRUPT_YIELD )`. The derived
surface depends on the PORT HEADER the demo files are compiled against, and
the derivation is allowed to notice that.

`kairos_capi.h` is generated at build time and never checked in. Two things
are checked about it, by the toolchain rather than by reading:

* **Compatibility.** `capi/header_gate.c` includes the ORACLE's real
  FreeRTOS headers and then ours, in one translation unit, so a declaration
  of ours that disagrees with FreeRTOS's is "conflicting types" and the
  build stops. It found four on its first run: a handle typed as a flag, a
  `TickType_t` declared as a POINTER where `timers.h` passes it by value, a
  `UBaseType_t` where the C says `BaseType_t`, and an `unsigned long` line
  number against a `uint32_t`.
* **Completeness.** The gate takes the address of every declared symbol
  into a table, so a declaration with no definition is an undefined
  reference at link time. Poison-tested: adding `xKairosPoisonNotDefined`
  produced `rust-lld: error: undefined symbol: xKairosPoisonNotDefined`.

The completeness half was **vacuous on its first attempt**, and only said
so when asked. `--gc-sections` discarded the address table because nothing
called it, and a discarded section's relocations are never resolved:
`llvm-nm` found no trace of it in the ELF. Making it reachable was not
enough either — the compiler proved every `&fn` non-null and folded the
walk to a constant, four bytes with no relocations. `volatile` is what
makes it real, and the table is now `0x160` bytes: 88 × 4, exactly.

### What the host port claims, and what it does not

`firmware/host-kernel` is the port's kill test: the M3 scheduling cell's
experiment moved off the chip. It asserts one thing the M3 cell could not,
because on a chip it is free: **exactly one task is on the CPU at a time**,
though every task is a real OS thread the operating system would happily
run in parallel.

Preemption is not optional and is not cooperative. `integer.c` and
`flop.c` never block and never yield; with `configUSE_PREEMPTION` set they
rely on being taken off the CPU. So the cell's low-priority task never
yields either, and the high task's lap count is the measurement:

| | switches | high-task laps | verdict |
|---|---:|---:|---|
| preemption on | 841 | 40 of an expected 40 | PASS |
| `KAIROS_HOST_NO_PREEMPT=1` | — | **1** of an expected 120 | FAIL, 3 checks |

Freezing an arbitrary thread is a famous way to deadlock, and the reason it
is safe here is narrow: the tick thread takes the critical-section lock
FIRST, and every kernel call and every heap call is inside that lock, so a
thread that is not holding it is not holding a lock of ours. What that does
not cover is a lock we do not own — the C runtime's allocator, or `stdout`
— and the demo files touch neither.

**The Unix backend is written too, and the kill test's "on Posix" clause is
now met on pthreads.** Unix has no call that stops another thread from
outside, so the thread freezes ITSELF: `freeze` sends `SIGUSR1` and the
handler parks the target in `sigsuspend` until `SIGUSR2` arrives — the
shape FreeRTOS's own Posix port uses. Two races had to be closed for that
to be safe, and both are in the handoff rather than the signal:
`pthread_kill` returns when a signal is QUEUED rather than handled, so
`freeze` does not answer until it can observe the target parked; and a thaw
arriving between "I am parked" and `sigsuspend` would be lost, so `SIGUSR2`
is blocked for the whole handler and arrives pending instead.
`rusty_rtos_port/docs/LEDGER.md` carries the detail and the poison row.

Measured on Linux (WSL2, Ubuntu, glibc, gcc), this cell, all 21 demo files:

| | result |
|---|---|
| 3,000 ticks, one check | **21/21**, `preemptive=true` |
| 30,000 ticks, 3 checks 10,000 apart | **21/21**, 3 runs of 3, 0 stream-buffer over-blocks |

**One compiler flag was needed, and it is upstream's conflict rather than
ours.** `MSVC-MingW/portmacro.h` defines `portYIELD_FROM_ISR( x )` as
`return x`; `MessageBufferAMP.c` calls it from a `static void` handler. MSVC
and clang accept returning a value from a void function with a warning, and
**GCC 14 made it an error**, so the same unmodified file that compiles on
the Windows host cell does not compile on the Linux one. Both files are
upstream's, the macro is upstream's, and the combination is one upstream
never builds — nobody compiles the AMP demo against the MinGW port. The
cell passes `-Wno-error=return-mismatch` and touches no demo source; a demo
*project* supplies build flags exactly as it supplies `FreeRTOSConfig.h`.
