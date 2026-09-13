// Under the `amp` feature the table holds ONE demo, so the declarations of
// the other files' entry points are unused. They are declarations of C
// functions that genuinely exist and are genuinely linked; the feature
// chooses which get CALLED, and deleting them per-feature would put a `cfg`
// on thirty-odd lines to silence a lint.
#![cfg_attr(feature = "amp", allow(dead_code))]

//! The demo files that RUN, as a table.
//!
//! **Shared by every C ABI cell.** The list of entry points, the checker
//! each file ships with, and the half of it that only runs from an
//! interrupt are properties of the DEMO FILES and not of the board -- so a
//! second copy in a second cell is how one of them ends up a demo short,
//! or calling an ISR half its owner did not start. That second failure is
//! not hypothetical: calling every ISR half unconditionally tripped
//! `QueueOverwrite.c:184` on every run that had not started it, which is
//! why `isr` is tied to its own demo here.
//!
//! A cell supplies nothing for this file. It is declarations and a table.
//!
//! # The widths are the C's, and they are not the same on every port
//!
//! These were written as `u32` and `i32` while the only cell was a
//! Cortex-M3, where `UBaseType_t` IS 32 bits. On a 64-bit host it is 64,
//! and a `u32` argument passed where the C reads a `UBaseType_t` leaves
//! the top half of the register undefined: `vStartGenericQueueTasks` was
//! handed a garbage priority, created `MuHigh` at 0 instead of 3, and the
//! demo asserted three hundred lines later that a task it thought was high
//! priority had not blocked. Nothing warned, because on the port they were
//! written for the two widths are the same.
//!
//! So every one of them is spelled as the typedef, which follows the
//! pointer width in `rusty_rtos_capi_core::ctypes` exactly as
//! `portmacro.h` makes it follow.

// The demos' own ISR-side functions, which several of them REQUIRE.
//
// This is not optional decoration. `IntSemTest`, `QueueSetPolling`,
// `EventGroupsDemo`, `TaskNotify` and the stream-buffer demos each have a
// half that only runs from an interrupt, and their checkers report failure
// if it never does. The real demo calls exactly these from
// `vFullDemoTickHookFunction` in `Demo/Posix_GCC/main_full.c`, and this is
// that list — so a demo failing here is failing on its own terms rather
// than for want of a harness.
unsafe extern "C" {

    fn vQueueOverwritePeriodicISRDemo();
    fn vQueueSetAccessQueueSetFromISR();
    fn vQueueSetPollingInterruptAccess();
    fn vPeriodicEventGroupsProcessing();
    fn vInterruptSemaphorePeriodicTest();
    fn xNotifyTaskFromISR();
    fn xNotifyArrayTaskFromISR();
    fn vPeriodicStreamBufferProcessing();
    fn vBasicStreamBufferSendFromISR();
    fn vTimerPeriodicISRTests();
}

unsafe extern "C" {
    /// Every demo file, compiled unmodified by `build.rs`.
    fn vCreateAbortDelayTasks();
    fn xAreAbortDelayTestTasksStillRunning() -> rusty_rtos_capi_core::ctypes::BaseType_t;
    fn vStartBlockingQueueTasks(uxPriority: rusty_rtos_capi_core::ctypes::UBaseType_t);
    fn xAreBlockingQueuesStillRunning() -> rusty_rtos_capi_core::ctypes::BaseType_t;
    fn vStartEventGroupTasks();
    fn xAreEventGroupTasksStillRunning() -> rusty_rtos_capi_core::ctypes::BaseType_t;
    fn vStartGenericQueueTasks(uxPriority: rusty_rtos_capi_core::ctypes::UBaseType_t);
    fn xAreGenericQueueTasksStillRunning() -> rusty_rtos_capi_core::ctypes::BaseType_t;
    fn vStartInterruptSemaphoreTasks();
    fn xAreInterruptSemaphoreTasksStillRunning() -> rusty_rtos_capi_core::ctypes::BaseType_t;
    fn vStartPolledQueueTasks(uxPriority: rusty_rtos_capi_core::ctypes::UBaseType_t);
    fn xArePollingQueuesStillRunning() -> rusty_rtos_capi_core::ctypes::BaseType_t;
    fn vStartQueuePeekTasks();
    fn xAreQueuePeekTasksStillRunning() -> rusty_rtos_capi_core::ctypes::BaseType_t;
    fn vStartQueueOverwriteTask(uxPriority: rusty_rtos_capi_core::ctypes::UBaseType_t);
    fn xIsQueueOverwriteTaskStillRunning() -> rusty_rtos_capi_core::ctypes::BaseType_t;
    fn vStartQueueSetTasks();
    fn xAreQueueSetTasksStillRunning() -> rusty_rtos_capi_core::ctypes::BaseType_t;
    fn vStartQueueSetPollingTask();
    fn xAreQueueSetPollTasksStillRunning() -> rusty_rtos_capi_core::ctypes::BaseType_t;
    fn vStartStreamBufferTasks();
    fn xAreStreamBufferTasksStillRunning() -> rusty_rtos_capi_core::ctypes::BaseType_t;
    fn vStartStreamBufferInterruptDemo();
    fn xIsInterruptStreamBufferDemoStillRunning() -> rusty_rtos_capi_core::ctypes::BaseType_t;
    fn vStartTaskNotifyTask();
    fn xAreTaskNotificationTasksStillRunning() -> rusty_rtos_capi_core::ctypes::BaseType_t;
    fn vStartTaskNotifyArrayTask();
    fn xAreTaskNotificationArrayTasksStillRunning() -> rusty_rtos_capi_core::ctypes::BaseType_t;
    fn vCreateBlockTimeTasks();
    fn xAreBlockTimeTestTasksStillRunning() -> rusty_rtos_capi_core::ctypes::BaseType_t;
    fn vStartCountingSemaphoreTasks();
    fn xAreCountingSemaphoreTasksStillRunning() -> rusty_rtos_capi_core::ctypes::BaseType_t;
    fn vCreateSuicidalTasks(uxPriority: rusty_rtos_capi_core::ctypes::UBaseType_t);
    fn xIsCreateTaskStillRunning() -> rusty_rtos_capi_core::ctypes::BaseType_t;
    fn vStartDynamicPriorityTasks();
    fn xAreDynamicPriorityTasksStillRunning() -> rusty_rtos_capi_core::ctypes::BaseType_t;
    fn vStartIntegerMathTasks(uxPriority: rusty_rtos_capi_core::ctypes::UBaseType_t);
    fn xAreIntegerMathsTaskStillRunning() -> rusty_rtos_capi_core::ctypes::BaseType_t;
    fn vStartRecursiveMutexTasks();
    fn xAreRecursiveMutexTasksStillRunning() -> rusty_rtos_capi_core::ctypes::BaseType_t;
    fn vStartSemaphoreTasks(uxPriority: rusty_rtos_capi_core::ctypes::UBaseType_t);
    #[cfg(feature = "amp")]
    fn vStartMessageBufferAMPTasks(xStackSize: rusty_rtos_capi_core::ctypes::StackDepth_t);
    #[cfg(feature = "amp")]
    fn xAreMessageBufferAMPTasksStillRunning() -> rusty_rtos_capi_core::ctypes::BaseType_t;
    fn vStartLEDFlashTasks(uxPriority: rusty_rtos_capi_core::ctypes::UBaseType_t);
    fn vAltStartComTestTasks(
        uxPriority: rusty_rtos_capi_core::ctypes::UBaseType_t,
        ulBaudRate: core::ffi::c_ulong,
        uxLED: rusty_rtos_capi_core::ctypes::UBaseType_t,
    );
    fn xAreComTestTasksStillRunning() -> rusty_rtos_capi_core::ctypes::BaseType_t;
    fn vStartMessageBufferTasks(xStackSize: rusty_rtos_capi_core::ctypes::StackDepth_t);
    fn xAreMessageBufferTasksStillRunning() -> rusty_rtos_capi_core::ctypes::BaseType_t;
    fn vStartMathTasks(uxPriority: rusty_rtos_capi_core::ctypes::UBaseType_t);
    fn xAreMathsTaskStillRunning() -> rusty_rtos_capi_core::ctypes::BaseType_t;
    fn vStartTimerDemoTask(xBasePeriodIn: rusty_rtos_capi_core::ctypes::TickType_t);
    fn xAreTimerDemoTasksStillRunning(
        xCycleFrequency: rusty_rtos_capi_core::ctypes::TickType_t,
    ) -> rusty_rtos_capi_core::ctypes::BaseType_t;
    fn xAreSemaphoreTasksStillRunning() -> rusty_rtos_capi_core::ctypes::BaseType_t;
}

/// One demo file: what starts it, and the checker IT ships with.
///
/// The verdict is always the demo's own function. This cell never decides
/// whether a demo is healthy -- that judgement belongs to the C, and using
/// ours instead would be marking our own homework.
pub(crate) struct Demo {
    pub(crate) name: &'static str,
    pub(crate) start: fn(),
    /// Ask this demo's own checker, given the number of ticks since the
    /// last time it was asked.
    ///
    /// Most checkers ignore the window: they answer "has my counter moved
    /// since you last looked", and that does not depend on how long it
    /// was. **`TimerDemo` is the exception, and it is why this argument
    /// exists.** Its checker verifies how many times an auto-reload timer
    /// fired, which is a RATE, so it has to be told the interval to judge
    /// against — `main_full.c` hands it the same `xCycleFrequency` its
    /// check task loops on.
    ///
    /// Giving every checker the argument, rather than reaching for a
    /// global that one of them reads, keeps the fact in the type where a
    /// reader trips over it. A window supplied at the wrong cadence makes
    /// `TimerDemo` report a failure that is really a harness bug, and that
    /// is exactly the class of thing this corpus exists not to do.
    /// `None` when the demo file ships **no checker at all**.
    ///
    /// Two of the thirty-four do: `flash.c` and `flash_timer.c` export a
    /// start function and nothing else — no `xAre...StillRunning`. There is
    /// no verdict to take from them, and writing one ourselves would be our
    /// opinion wearing the demo's clothes, which is the one thing this
    /// corpus exists not to do. So they run, and they are reported
    /// separately from any N-of-N claim.
    pub(crate) check: Option<
        fn(rusty_rtos_capi_core::ctypes::TickType_t) -> rusty_rtos_capi_core::ctypes::BaseType_t,
    >,
    /// The half that only runs from an interrupt, if this demo has one.
    ///
    /// Several demos REQUIRE it: their checker reports failure if the ISR
    /// side never runs. The real demo calls exactly these from
    /// `vFullDemoTickHookFunction`. Tying each to its own demo rather than
    /// calling them all from the tick is not tidiness — a demo that was
    /// never started has no queues, and its ISR function reaches them
    /// anyway. Calling them unconditionally tripped
    /// `QueueOverwrite.c:184` on every run that did not start it.
    pub(crate) isr: Option<fn()>,
}

/// Run one demo instead of all of them, named at compile time:
///
/// ```sh
/// KAIROS_CAPI_ONLY=BlockQ cargo run --release
/// ```
///
/// Demo files share a kernel, and sharing means interfering: a task that
/// never blocks starves every lower-priority task in every OTHER demo, and
/// the result is six checkers reporting NO for one demo's behaviour. This
/// is how a per-demo verdict is obtained before a combined one is trusted.
pub(crate) const ONLY: Option<&str> = option_env!("KAIROS_CAPI_ONLY");

/// Is this demo in the filter?
///
/// The filter is a COMMA-SEPARATED set, not one name. One name answers
/// "does this demo work alone"; a set answers "which other demo is it
/// that breaks it", and that is the question an
/// individually-pass/together-fail leaves you with. Bisecting twenty-one
/// files by hand is five runs; bisecting them one at a time is twenty.
#[must_use]
pub(crate) fn selected(name: &str) -> bool {
    let Some(only) = ONLY else {
        return true;
    };
    only.split(',').any(|want| want.trim() == name)
}

/// The priorities are **the real demo's**, from `Demo/Posix_GCC/main_full.c`:
///
/// ```text
/// mainQUEUE_POLL_PRIORITY   tskIDLE_PRIORITY + 1
/// mainSEM_TEST_PRIORITY     tskIDLE_PRIORITY + 1
/// mainBLOCK_Q_PRIORITY      tskIDLE_PRIORITY + 2
/// mainINTEGER_TASK_PRIORITY tskIDLE_PRIORITY
/// mainCHECK_TASK_PRIORITY   configMAX_PRIORITIES - 2   (this cell's monitor)
/// ```
///
/// Inventing them was a mistake worth recording. With PollQ, BlockQ and
/// semtest all at 2 and integer at 1, the six demos passed INDIVIDUALLY and
/// four of six failed together, with switches collapsing from 3,057 to 121:
/// tasks that never block starve everything beneath them, and that is
/// correct fixed-priority scheduling doing its job on a bad assignment.
/// These files are designed to be run together at THESE numbers.
/// Under the `amp` feature the table is **only** `MessageBufferAMP`.
///
/// Not a convenience: `sbSEND_COMPLETED` is in force, so every other
/// stream-buffer demo in this table would behave differently and two would
/// reach a control buffer that does not exist. Running them beside it would
/// not be a stronger test, it would be a wrong one.
#[cfg(feature = "amp")]
pub(crate) static DEMOS: &[Demo] = &[Demo {
    // `256` is `configMINIMAL_STACK_SIZE`, which is what `main_full.c`
    // passes. The demo pretends to be two cores: "core A" sends through a
    // message buffer, the completed send is routed to
    // `vGenerateCoreBInterrupt` by the cell's `send_completed` hook, and
    // "core B" echoes back.
    name: "MessageBufferAMP",
    start: || unsafe { vStartMessageBufferAMPTasks(256) },
    check: Some(|_| unsafe { xAreMessageBufferAMPTasksStillRunning() }),
    isr: None,
}];

#[cfg(not(feature = "amp"))]
pub(crate) static DEMOS: &[Demo] = &[
    Demo {
        name: "AbortDelay",
        start: || unsafe { vCreateAbortDelayTasks() },
        check: Some(|_| unsafe { xAreAbortDelayTestTasksStillRunning() }),
        isr: None,
    },
    Demo {
        name: "BlockQ",
        start: || unsafe { vStartBlockingQueueTasks(2) },
        check: Some(|_| unsafe { xAreBlockingQueuesStillRunning() }),
        isr: None,
    },
    Demo {
        name: "EventGroups",
        start: || unsafe { vStartEventGroupTasks() },
        check: Some(|_| unsafe { xAreEventGroupTasksStillRunning() }),
        isr: Some(|| unsafe { vPeriodicEventGroupsProcessing() }),
    },
    Demo {
        name: "GenQTest",
        start: || unsafe { vStartGenericQueueTasks(0) },
        check: Some(|_| unsafe { xAreGenericQueueTasksStillRunning() }),
        isr: None,
    },
    Demo {
        name: "IntSemTest",
        start: || unsafe { vStartInterruptSemaphoreTasks() },
        check: Some(|_| unsafe { xAreInterruptSemaphoreTasksStillRunning() }),
        isr: Some(|| unsafe { vInterruptSemaphorePeriodicTest() }),
    },
    Demo {
        name: "PollQ",
        start: || unsafe { vStartPolledQueueTasks(1) },
        check: Some(|_| unsafe { xArePollingQueuesStillRunning() }),
        isr: None,
    },
    Demo {
        name: "QPeek",
        start: || unsafe { vStartQueuePeekTasks() },
        check: Some(|_| unsafe { xAreQueuePeekTasksStillRunning() }),
        isr: None,
    },
    Demo {
        name: "QueueOverwrite",
        start: || unsafe { vStartQueueOverwriteTask(0) },
        check: Some(|_| unsafe { xIsQueueOverwriteTaskStillRunning() }),
        isr: Some(|| unsafe { vQueueOverwritePeriodicISRDemo() }),
    },
    Demo {
        name: "QueueSet",
        start: || unsafe { vStartQueueSetTasks() },
        check: Some(|_| unsafe { xAreQueueSetTasksStillRunning() }),
        isr: Some(|| unsafe { vQueueSetAccessQueueSetFromISR() }),
    },
    Demo {
        name: "QueueSetPolling",
        start: || unsafe { vStartQueueSetPollingTask() },
        check: Some(|_| unsafe { xAreQueueSetPollTasksStillRunning() }),
        isr: Some(|| unsafe { vQueueSetPollingInterruptAccess() }),
    },
    Demo {
        name: "StreamBuffer",
        start: || unsafe { vStartStreamBufferTasks() },
        check: Some(|_| unsafe { xAreStreamBufferTasksStillRunning() }),
        isr: Some(|| unsafe { vPeriodicStreamBufferProcessing() }),
    },
    Demo {
        name: "StreamBufInt",
        start: || unsafe { vStartStreamBufferInterruptDemo() },
        check: Some(|_| unsafe { xIsInterruptStreamBufferDemoStillRunning() }),
        isr: Some(|| unsafe { vBasicStreamBufferSendFromISR() }),
    },
    Demo {
        name: "TaskNotify",
        start: || unsafe { vStartTaskNotifyTask() },
        check: Some(|_| unsafe { xAreTaskNotificationTasksStillRunning() }),
        isr: Some(|| unsafe { xNotifyTaskFromISR() }),
    },
    Demo {
        name: "TaskNotifyArray",
        start: || unsafe { vStartTaskNotifyArrayTask() },
        check: Some(|_| unsafe { xAreTaskNotificationArrayTasksStillRunning() }),
        // A DELIBERATE deviation from `main_full.c`, and the only one.
        //
        // Upstream's authoritative demo comments out all three of this
        // file's lines -- `vStartTaskNotifyArrayTask()`,
        // `xAreTaskNotificationArrayTasksStillRunning()` and
        // `xNotifyArrayTaskFromISR()` -- so it does not run this demo at
        // all. We do run it, and a demo run without the ISR half it was
        // written against blocks forever waiting for a notification
        // nothing will send. Copying upstream's commented-out ISR line
        // while uncommenting its start line is not fidelity, it is half a
        // configuration.
        isr: Some(|| unsafe { xNotifyArrayTaskFromISR() }),
    },
    Demo {
        name: "blocktim",
        start: || unsafe { vCreateBlockTimeTasks() },
        check: Some(|_| unsafe { xAreBlockTimeTestTasksStillRunning() }),
        isr: None,
    },
    Demo {
        name: "countsem",
        start: || unsafe { vStartCountingSemaphoreTasks() },
        check: Some(|_| unsafe { xAreCountingSemaphoreTasksStillRunning() }),
        isr: None,
    },
    Demo {
        name: "death",
        start: || unsafe { vCreateSuicidalTasks(3) },
        check: Some(|_| unsafe { xIsCreateTaskStillRunning() }),
        isr: None,
    },
    Demo {
        name: "dynamic",
        start: || unsafe { vStartDynamicPriorityTasks() },
        check: Some(|_| unsafe { xAreDynamicPriorityTasksStillRunning() }),
        isr: None,
    },
    Demo {
        name: "integer",
        start: || unsafe { vStartIntegerMathTasks(0) },
        check: Some(|_| unsafe { xAreIntegerMathsTaskStillRunning() }),
        isr: None,
    },
    Demo {
        name: "recmutex",
        start: || unsafe { vStartRecursiveMutexTasks() },
        check: Some(|_| unsafe { xAreRecursiveMutexTasksStillRunning() }),
        isr: None,
    },
    Demo {
        name: "semtest",
        start: || unsafe { vStartSemaphoreTasks(1) },
        check: Some(|_| unsafe { xAreSemaphoreTasksStillRunning() }),
        isr: None,
    },
    Demo {
        // A loopback serial port: everything transmitted comes back, which
        // is what `comtest.c` asks for in words — "a loopback connector
        // should be used so that everything that is transmitted is
        // received". The wire is a kernel queue, in `seam/board.rs`.
        //
        // **`comtest_strings.c` is NOT here and cannot be.** It is the
        // alternative version of this file and defines the same checker,
        // `xAreComTestTasksStillRunning`, so linking both is a duplicate
        // definition — the second mutually exclusive PAIR in this corpus
        // after `flop`/`sp_flop`, and another reason no single binary can
        // hold all 34 files.
        //
        // `2` is the priority `main.c` in the reference projects uses for
        // the receive task (the transmit task is created one below it);
        // `38400` is the usual baud, ignored by a loopback with no wire to
        // clock; `4` is the first of the two LEDs it drives, chosen above
        // `flash.c`'s three so the counts stay readable.
        name: "comtest",
        start: || unsafe { vAltStartComTestTasks(2, 38_400, 4) },
        check: Some(|_| unsafe { xAreComTestTasksStillRunning() }),
        isr: None,
    },
    Demo {
        // Three tasks, one LED each, on `xTaskDelayUntil`.
        //
        // **No checker.** This file exports `vStartLEDFlashTasks` and
        // nothing else, so there is no verdict of its own to report and
        // none is invented. What it proves is that `xTaskDelayUntil` and
        // three independent periodic tasks run without faulting; the LED
        // toggle counts in the run report are the evidence, and they are
        // OURS, not the demo's.
        name: "flash",
        start: || unsafe { vStartLEDFlashTasks(0) },
        check: None,
        isr: None,
    },
    // `flash_timer.c` IS NOT HERE, and the reason is a constraint between
    // two demo files rather than a defect in either.
    //
    // `TimerDemo.c`'s `prvTest1_CreateTimersWithoutSchedulerRunning` runs
    // before the scheduler starts, deliberately fills the timer command
    // queue by starting exactly `configTIMER_QUEUE_LENGTH` timers, and
    // asserts that every one of those starts SUCCEEDS (line 310) before
    // requiring the next to fail. `vStartLEDFlashTimers` also starts its
    // timers in that window, so it takes slots TimerDemo has counted on:
    // with three LEDs it left seven, and `TimerDemo.c:313` failed.
    //
    // Raising `configTIMER_QUEUE_LENGTH` does not help, which is the
    // interesting part — TimerDemo sizes its own test FROM that macro, so
    // it always fills exactly the queue there is. Any demo that starts a
    // timer before the scheduler runs is incompatible with it, whatever
    // the queue length.
    //
    // So this is a THIRD kind of mutual exclusion, after `flop`/`sp_flop`
    // and `comtest`/`comtest_strings` (duplicate symbols) and
    // `MessageBufferAMP` (a global `sbSEND_COMPLETED`). TimerDemo wins the
    // slot: it has a checker and covers a whole subsystem, where
    // `flash_timer.c` ships no checker at all. It belongs in a cell of its
    // own.
    Demo {
        // The echo server that creates and DELETES a message buffer on
        // every loop, which is why it needs an arena that can hand the
        // same bytes back. `256` is `configMINIMAL_STACK_SIZE`, which is
        // what `main_full.c` passes; it counts WORDS, so it is a kilobyte
        // on the chip and two on a 64-bit host.
        //
        // No ISR half: it is not in `vFullDemoTickHookFunction`, and the
        // stream-buffer ISR work belongs to `StreamBuffer` and
        // `StreamBufInt` instead.
        name: "MessageBuffer",
        start: || unsafe { vStartMessageBufferTasks(256) },
        check: Some(|_| unsafe { xAreMessageBufferTasksStillRunning() }),
        isr: None,
    },
    Demo {
        // Four floating-point tasks that NEVER block and never yield, at
        // the idle priority — `mainFLOP_TASK_PRIORITY` is
        // `tskIDLE_PRIORITY` in `main_full.c`, and that is the point of
        // them: they exist to be preempted, and a port that cannot do it
        // starves everything at their priority.
        //
        // **`sp_flop.c` is NOT here and cannot be.** It is the
        // single-precision ALTERNATIVE to this file and defines the same
        // two symbols, `vStartMathTasks` and `xAreMathsTaskStillRunning`.
        // Linking both is a duplicate definition, so no single binary can
        // ever run all 34 demo files — one of this pair has to be chosen,
        // exactly as `MessageBufferAMP` needs a binary of its own. The
        // double-precision file is the one `main_full.c` includes.
        name: "flop",
        start: || unsafe { vStartMathTasks(0) },
        check: Some(|_| unsafe { xAreMathsTaskStillRunning() }),
        isr: None,
    },
    Demo {
        // The software timers, and the one demo whose checker is a RATE.
        //
        // `50` is `mainTIMER_TEST_PERIOD` from `main_full.c`, and it is
        // not a free choice: the file builds `configTIMER_QUEUE_LENGTH`
        // auto-reload timers at multiples of this period and then asserts
        // on how many times each fired. It creates FOURTEEN timers with
        // the queue length at 10 — one one-shot, eleven auto-reload, two
        // driven from the ISR — which is why both cells' `TIMERS` arena
        // had to grow to hold it beside the four the other demos make.
        name: "TimerDemo",
        start: || unsafe { vStartTimerDemoTask(50) },
        check: Some(|window| unsafe { xAreTimerDemoTasksStillRunning(window) }),
        isr: Some(|| unsafe { vTimerPeriodicISRTests() }),
    },
];
