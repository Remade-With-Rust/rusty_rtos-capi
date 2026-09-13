//! **K6's other half: the same unmodified C demo files, on a laptop.**
//!
//! `firmware/mps2-an385-qemu-capi` runs them on a Cortex-M3 under QEMU.
//! This cell runs *the same files*, through *the same seam*, against *the
//! same kernel*, on OS threads. The two share `seam/abi.rs` and
//! `seam/demos.rs` byte for byte and differ only in the port beneath them
//! and the four verbs that port implies.
//!
//! # Why this took a new port to be possible at all
//!
//! The kill test for K6 reads "the unmodified C standard demo tasks link
//! against `rusty_rtos-capi` and pass **on Posix, then on QEMU M3**", and
//! the order came out inverted. The Kairos kernel is stackless: a blocking
//! call answers `Wait::Blocked`, meaning "call again when this task next
//! runs". A C task cannot do that — `vTaskDelay` must return *later*, with
//! its locals intact — so it needs a real stack, and there was no host
//! port that had any. The three real ports were bare-metal and the sim
//! port was stackless, so M3 was where this could start.
//!
//! `rusty_rtos_port-host` is what closes it: one OS thread per task, a
//! single run permit, and a tick thread that can freeze whichever thread
//! holds it. The stacks are the operating system's.
//!
//! # What is different, and what deliberately is not
//!
//! | | QEMU M3 | here |
//! |---|---|---|
//! | a task's stack | a static array plus `init_stack` | an OS thread |
//! | the tick | `SysTick` | a thread that sleeps a millisecond |
//! | a switch | `PendSV` | the run permit, or a freeze |
//! | `BaseType_t` | `long`, 32 bits | `long long`, 64 bits |
//! | the seam | `seam/abi.rs` | `seam/abi.rs` |
//! | the demo files | the oracle's, unmodified | the oracle's, unmodified |
//! | the checkers | the demos' own | the demos' own |
//!
//! The `BaseType_t` row is the interesting one: it is a PORT fact, the two
//! cells disagree about it, and neither the seam nor the demo files needed
//! a line changed for that — because both are written in terms of the
//! typedef rather than in terms of a width.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use rusty_rtos_core::config::Config;
use rusty_rtos_core::handle::TaskHandle;
use rusty_rtos_core::tick::Bits32;
use rusty_rtos_core::trace::{Event, Trace};
use rusty_rtos_kernel_core::queue::Wait;
use rusty_rtos_kernel_core::{items_for, lists_for, Kernel};
use rusty_rtos_port_host::{
    init_task, set_scheduler, start_first_task, HostPort, Ticker, CURRENT, PREEMPTIVE,
};

/// The seam, shared with every other C ABI cell.
#[path = "../../../seam/abi.rs"]
mod abi;

/// The demo files and their table, shared with every other C ABI cell.
/// The demo PROJECT's board drivers: LEDs and a loopback serial port.
///
/// Not ABI symbols and deliberately not in the generated header — see the
/// module's own explanation. `seam/board_gate.c` audits their signatures
/// against the oracle's `partest.h` and `serial.h` instead.
#[path = "../../../seam/board.rs"]
mod board;

#[path = "../../../seam/demos.rs"]
mod demos;
pub(crate) use demos::DEMOS;

/// The same numbers the QEMU cell's `CapiConfig` carries, and they must be
/// the same: `FreeRTOSConfig.h` and this describe ONE system, and a
/// disagreement compiles cleanly on both sides.
#[derive(Debug, Clone, Copy, Default)]
pub struct CapiConfig;

impl Config for CapiConfig {
    type Tick = Bits32;
    const TICK_RATE_HZ: u32 = 1000;
    const MAX_PRIORITIES: u8 = 5;
    const MINIMAL_STACK_SIZE: usize = 256;
    const MAX_TASK_NAME_LEN: usize = 16;
    const TIMER_TASK_PRIORITY: u8 = 2;
    const TIMER_TASK_STACK_DEPTH: usize = 256;
    const TIMER_QUEUE_LENGTH: usize = 10;
    const NOTIFICATION_ARRAY_ENTRIES: usize = 3;
    // `sizeof( configMESSAGE_BUFFER_LENGTH_TYPE )`, which the C defaults to
    // `size_t`. Read from `ctypes` rather than written as a number: the
    // trait's default of 4 is right on the chip and four bytes wrong on a
    // 64-bit host, and `MessageBufferDemo.c:262` compares our free space
    // against its own arithmetic and fails on the first message.
    const MESSAGE_LENGTH_BYTES: usize = rusty_rtos_capi_core::ctypes::MESSAGE_LENGTH_BYTES;
}

/// This cell measures whether the C file runs, not what it traced.
#[derive(Debug, Default)]
struct NoTrace;
impl Trace for NoTrace {
    fn event(&mut self, _tick: u64, _event: Event<'_>) {}
}

pub(crate) const TASKS: usize = 96;
pub(crate) const QUEUES: usize = 48;
const SLOTS: usize = 1024;
// Four for the other demos, FOURTEEN for `TimerDemo` alone
// (`configTIMER_QUEUE_LENGTH + 1` auto-reload, one one-shot, two from the
// ISR), and room to see an over-run rather than exhaust the arena.
pub(crate) const TIMERS: usize = 24;
const GROUPS: usize = 16;
const BUFFERS: usize = 16;
const BYTES: usize = 8192;

/// `usStackDepth` ceiling, in stack WORDS.
///
/// The host port gives every task the same OS stack, so this is a bound the
/// seam checks rather than an arena it carves. `xTaskCreate` refuses a
/// larger request instead of quietly giving less, exactly as on the chip.
pub(crate) const STACK_WORDS: usize = 512;

/// The tick hook, which is `CapiTickHook` on the chip too: it exists for
/// one method.
///
/// `xEventGroupSetBitsFromISR` cannot walk a waiting list from an
/// interrupt, so the kernel defers it to the daemon task, which then asks
/// the application's `TickHook::pended` what to do with it. With
/// `NoTickHook` the deferral is accepted, counted as `pdPASS`, and
/// dropped.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CapiTickHook;

impl rusty_rtos_core::hooks::TickHook<K> for CapiTickHook {
    fn tick(self, _kernel: &mut K) -> Self {
        self
    }

    fn timer(
        _kernel: &mut K,
        timer: rusty_rtos_core::handle::TimerHandle,
        _callback: u16,
        _id: u64,
    ) {
        // Recorded, not run: the kernel is borrowed here, and the C
        // callback calls straight back into it. The daemon task drains
        // these outside the borrow. See `abi::note_timer_expiry`.
        abi::note_timer_expiry(timer);
    }

    /// `sbSEND_COMPLETED`, for the AMP demo only.
    ///
    /// The C overrides a macro in `stream_buffer.c`. We do not compile that
    /// file — this kernel decides what a completed send does — so the
    /// override lands here instead, and the contract is the same one:
    /// answering `true` means "handled", and the kernel does NOT go on to
    /// notify the task waiting on the buffer.
    ///
    /// `MessageBufferAMP.c` uses that to pretend to be two cores: instead
    /// of a notification, the send posts the buffer's handle to a control
    /// buffer and calls what stands in for the other core's interrupt.
    ///
    /// Behind a feature because it is GLOBAL. Every stream-buffer send in
    /// the process would route here, and `vGenerateCoreBInterrupt` reaches
    /// a control buffer that exists only after the AMP demo has started —
    /// so with this on, `StreamBuffer` and `MessageBuffer` are not merely
    /// different, they are wrong. Hence a second binary.
    #[cfg(feature = "amp")]
    fn send_completed(
        _kernel: &mut K,
        buffer: rusty_rtos_core::handle::StreamBufferHandle,
    ) -> bool {
        // SAFETY: the C's own function, given the handle it gave us.
        unsafe { vGenerateCoreBInterrupt(abi::handle_to_c(buffer)) };
        true
    }

    fn pended(kernel: &mut K, function: u16, param1: u64, param2: u64) {
        if matches!(
            function,
            rusty_rtos_kernel_core::events::PENDED_SET_BITS
                | rusty_rtos_kernel_core::events::PENDED_CLEAR_BITS
        ) {
            let _ = kernel.event_group_pended_call(function, param1, param2);
        }
    }
}

pub(crate) type K = Kernel<
    CapiConfig,
    HostPort,
    NoTrace,
    CapiTickHook,
    TASKS,
    { items_for(TASKS, TIMERS) },
    { lists_for(CapiConfig::MAX_PRIORITIES, QUEUES, GROUPS) },
    QUEUES,
    SLOTS,
    BUFFERS,
    BYTES,
    TIMERS,
    GROUPS,
>;

struct KernelCell(std::cell::UnsafeCell<Option<K>>);
// SAFETY: every access goes through `with_kernel`, which takes the port's
// critical section -- the same lock the tick thread takes before it touches
// anything, and the same lock that makes freezing a task thread safe.
unsafe impl Sync for KernelCell {}
static KERNEL: KernelCell = KernelCell(std::cell::UnsafeCell::new(None));

static PORT: HostPort = HostPort::new();

// ------------------------------- the four verbs the shared seam asks for --
//
// `seam/abi.rs` names no chip and no host. These four are the whole of the
// difference between this cell and the Cortex-M3 one.

/// `yield_now`: leave the CPU and come back when the kernel says so.
pub(crate) fn yield_now() {
    rusty_rtos_port_host::pend_switch();
}

/// `without_interrupts`: run `f` with nothing able to preempt it.
///
/// On the chip this is `cpsid i`. Here it is the port's critical lock,
/// which the tick thread takes before it touches the kernel and before it
/// freezes anybody -- so holding it is exactly "an interrupt cannot land
/// here", stated in the only terms a host has.
pub(crate) fn without_interrupts<R>(f: impl FnOnce() -> R) -> R {
    use rusty_rtos_core::port::Port as _;
    PORT.enter_critical();
    let out = f();
    PORT.exit_critical();
    out
}

/// `note`: say something to whoever is watching.
///
/// It goes to stderr and it goes inside the critical section. Both matter:
/// `stdout` has a lock this port does not own, and a task frozen while
/// holding it would deadlock the next task that printed. Inside the
/// critical section no freeze can happen.
#[macro_export]
macro_rules! note {
    ($($arg:tt)*) => {
        $crate::without_interrupts(|| {
            // The OS thread's name, because on this port "which task is
            // running" has two answers -- the kernel's and the operating
            // system's -- and a diagnostic that only prints one cannot
            // show them disagreeing.
            let t = ::std::thread::current();
            ::std::eprint!("[{}] ", t.name().unwrap_or("main"));
            ::std::eprintln!($($arg)*)
        })
    };
}

/// `die`: stop, with a failing status.
pub(crate) fn die() -> ! {
    std::process::exit(1)
}

// `reconcile()` used to live here: a function that checked whether the
// kernel had changed its mind about the running task and yielded to catch
// up. It was a workaround for `HostPort` not declaring
// `Port::COMMITS_SWITCH`, which made the kernel move `current` itself and
// leave this port a step behind -- "code runs as a task the kernel no
// longer thinks is current", exactly as the trait's own doc warns.
//
// With the port declaring it properly there is nothing to reconcile: the
// decision and the change of running task are one step. Removed rather
// than kept "just in case", because a workaround left beside its own fix
// is the thing that makes the next person distrust both. `check_identity`
// below stays: it is the ASSERTION that none of this is needed, and it
// costs a thread-local read.

/// `where_it_runs`: where task `i` will resume, or 0 if nowhere yet.
///
/// On this cell that is its OS thread: the port hands one out when the
/// task is armed, and zero means the slot has an entry point and nowhere
/// to run it.
pub(crate) fn where_it_runs(i: usize) -> usize {
    rusty_rtos_port_host::thread_of(i) as usize
}

/// Borrow the kernel inside the port's critical section.
pub(crate) fn with_kernel<R>(f: impl FnOnce(&mut K) -> R) -> Option<R> {
    without_interrupts(|| {
        // SAFETY: the critical section is held, so the tick thread is not
        // inside the kernel and no other task thread holds the run permit.
        let slot = unsafe { &mut *KERNEL.0.get() };
        check_identity(slot.as_ref());
        slot.as_mut().map(f)
    })
}

/// The kernel's idea of the running task must be the thread that is
/// running. Nothing else is true on a chip, and it has to be made true
/// here.
///
/// Every FreeRTOS API that takes `NULL` to mean "the calling task" --
/// `vTaskPrioritySet`, `uxTaskPriorityGet`, `vTaskSuspend`,
/// `vTaskDelete` -- resolves it through the kernel's `current`. If that
/// has drifted from the thread actually executing, those calls name
/// somebody else, silently, and the damage shows up much later as a
/// scheduling assertion in a demo that has nothing to do with the call.
fn check_identity(kernel: Option<&K>) {
    let (Some(kernel), Some(mine)) = (kernel, rusty_rtos_port_host::my_index()) else {
        return;
    };
    let believes = usize::from(kernel.current().index());
    // Once, and loudly. It costs a thread-local read and a compare, and it
    // is the one invariant this port exists to keep: `reconcile` is what
    // keeps it, and a report here means `reconcile` has a hole.
    if believes != mine && !IDENTITY_BROKEN.swap(true, Ordering::SeqCst) {
        let (holder, depth) = rusty_rtos_port_host::critical_depth();
        eprintln!(
            "[{}] IDENTITY: this thread is task {mine}, the kernel believes {believes} is              running (critical: mine={holder} depth={depth}, switch pending={})",
            std::thread::current().name().unwrap_or("main"),
            rusty_rtos_port_host::switch_pending()
        );
    }
}

static IDENTITY_BROKEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// As `with_kernel`, for a caller that already holds the critical section.
fn with_kernel_locked<R>(f: impl FnOnce(&mut K) -> R) -> Option<R> {
    // SAFETY: the caller holds the port's critical section.
    let slot = unsafe { &mut *KERNEL.0.get() };
    slot.as_mut().map(f)
}

/// Give one task somewhere to run.
///
/// This is the chip cell's `arm_task`, and the difference is the whole
/// point of the port: there, a static array and an initial frame; here, an
/// OS thread, parked until it is granted the run permit.
pub(crate) fn arm_task(handle: TaskHandle, entry: extern "C" fn(usize) -> !) -> bool {
    let i = usize::from(handle.index());
    if i >= TASKS || i >= rusty_rtos_port_host::MAX_TASKS {
        return false;
    }
    init_task(i, entry);
    true
}

static SWITCHES: AtomicU32 = AtomicU32::new(0);

/// Turns on the CPU, per task slot.
#[allow(clippy::declare_interior_mutable_const)]
const NO_TURNS: AtomicU32 = AtomicU32::new(0);
static TURNS: [AtomicU32; TASKS] = [NO_TURNS; TASKS];

/// Every task that ever ran, with its share of the switches.
fn report_turns() {
    let total = SWITCHES.load(Ordering::Relaxed).max(1);
    println!("CAPI turns on the CPU, per task:");
    for i in 0..TASKS {
        let turns = TURNS[i].load(Ordering::Relaxed);
        if turns == 0 {
            continue;
        }
        let Some(Some(handle)) = with_kernel(|k| k.task_at(i)) else {
            continue;
        };
        let name = with_kernel(|k| k.name_of(handle)).and_then(|r| r.ok());
        let priority = with_kernel(|k| k.priority_of(Some(handle))).and_then(|r| r.ok());
        println!(
            "    {:<3} {:<18} priority {:?}  {:>10} turns  {:>5.1}%",
            i,
            name.as_ref()
                .map_or("?", rusty_rtos_kernel_core::name::Name::as_str),
            priority,
            turns,
            f64::from(turns) * 100.0 / f64::from(total)
        );
    }
}
static TICK_HOOKS: AtomicU64 = AtomicU64::new(0);

/// The port asks who is next; the kernel answers.
extern "C" fn pick_next() {
    let before = CURRENT.load(Ordering::SeqCst);
    let next = with_kernel_locked(|k| {
        k.switch_context();
        k.current()
    });
    if let Some(handle) = next {
        let to = usize::from(handle.index());
        if probe_switches() && to != before {
            let t = std::thread::current();
            eprintln!(
                "[{}] switch {} -> {}",
                t.name().unwrap_or("main"),
                before,
                to
            );
        }
        CURRENT.store(to, Ordering::SeqCst);
        SWITCHES.fetch_add(1, Ordering::Relaxed);
        // Who is actually getting the CPU. A demo that "does nothing" is
        // either blocked or starved, and a per-task turn count is the only
        // thing that tells those apart -- a global switch rate can be
        // perfectly healthy while one task takes all of it.
        if let Some(c) = TURNS.get(to) {
            c.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn probe_switches() -> bool {
    option_env!("KAIROS_CAPI_PROBE") == Some("states")
}

/// One tick, from the tick thread, with the critical section held and the
/// running task frozen.
///
/// This is where `vApplicationTickHook` goes: every demo's ISR half runs
/// here, which is where the real demo calls them from
/// (`vFullDemoTickHookFunction`). Several demos' checkers report failure
/// if their ISR half never runs.
extern "C" fn on_tick() -> bool {
    let want = with_kernel_locked(|k| k.increment_tick()).unwrap_or(false);
    if STARTED.load(Ordering::Relaxed) {
        TICK_HOOKS.fetch_add(1, Ordering::Relaxed);
        for demo in DEMOS {
            if !demos::selected(demo.name) {
                continue;
            }
            if let Some(isr) = demo.isr {
                isr();
            }
        }
    }
    // `portYIELD_FROM_ISR( xHigherPriorityTaskWoken )`, for the callers
    // that passed NULL. `increment_tick` was asked BEFORE the ISR halves
    // ran, so a task one of them woke is not in `want` -- and a wake-up
    // dropped here is a task that stays ready and unscheduled until
    // something else happens to yield.
    want || abi::take_isr_woke()
}

static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// How long the C demos run before they are asked whether they are healthy.
///
/// Settable with `KAIROS_CAPI_TICKS`, because the right answer is not the
/// same on every port. A demo's checker asks "has your counter moved since
/// last time", and a task that has not been scheduled yet has not moved --
/// so the run has to be long enough for the SLOWEST starter among sixty
/// tasks, not for the average one.
/// The default is ONE FULL CHECK PERIOD, and that is the demos' number.
///
/// It was 3,000 ticks, which is less than the 10,000 every checker in this
/// corpus is written against ([`CHECK_PERIOD_TICKS`]) — so the default run
/// asked "has your counter moved" before any demo had been given a period
/// to move it in. That is not merely the weak form of the question; it
/// produces FALSE FAILURES, and one arrived the moment `flop.c` joined the
/// set: four never-blocking tasks at the idle priority, which is where
/// `TaskNotifyArray`'s task also sits, so it got a fifth of the CPU it
/// used to and needed more than 3,000 ticks to finish one test pass. It
/// reported FAIL at 3,000 and PASSES three times over at 30,000.
///
/// A fast signal that calls a healthy demo broken is worse than a slow
/// one. The cost is wall-clock in the emulated cell and nothing else.
const RUN_TICKS: u64 = match option_env!("KAIROS_CAPI_TICKS") {
    Some(s) => match u64::from_str_radix(s, 10) {
        Ok(n) if n > 0 => n,
        _ => CHECK_PERIOD_TICKS,
    },
    None => CHECK_PERIOD_TICKS,
};

/// A check pattern as `ok`/`NO`, oldest first.
fn pattern(bits: u64) -> heapless_pattern::Pattern {
    heapless_pattern::Pattern(bits, SAMPLES)
}

/// A tiny formatter, because a cell that is `no_std` on one port cannot
/// build a `String` on the other and still share its seam.
mod heapless_pattern {
    /// `bits`, lowest first, as `ok`/`NO` separated by spaces.
    pub struct Pattern(pub u64, pub u64);

    impl core::fmt::Display for Pattern {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            for n in 0..self.1 {
                if n > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{}", if self.0 & (1 << n) != 0 { "ok" } else { "NO" })?;
            }
            Ok(())
        }
    }
}

/// How long between asking each demo's checker whether it is still
/// running, and it is the DEMO's number, not ours.
///
/// `Demo/Posix_GCC/main_full.c`'s check task:
///
/// ```c
/// const TickType_t xCycleFrequency = pdMS_TO_TICKS( 10000UL );
/// ```
///
/// "Has your counter moved since last time" is a question whose answer
/// depends entirely on how long "last time" was. Asked thirteen times too
/// fast it reported seven healthy demos as stalled — several of them have
/// cycles measured in thousands of ticks, and twenty-one of them share one
/// kernel here.
///
/// Asking more than once matters: a single call at the end only asks "did
/// this counter EVER move", and `xAreTaskNotificationArrayTasksStillRunning`
/// only exercises its course-cycle branch every THIRD call, so one call
/// never runs that half at all.
const CHECK_PERIOD_TICKS: u64 = 10_000;

/// How many times the checkers are asked: as many full periods as the run
/// length allows, and at least one.
///
/// A run shorter than one period asks once at the end, which is the weak
/// form — the report says how many checks each demo survived so a reader
/// can tell which question was asked.
const SAMPLES: u64 = if RUN_TICKS / CHECK_PERIOD_TICKS > 0 {
    RUN_TICKS / CHECK_PERIOD_TICKS
} else {
    1
};

/// Ask every demo's checker once.
///
/// `survived[i]` counts the checks a demo has passed IN A ROW from the
/// start; `failed_at[i]` is the first check it failed, or 0. Which check
/// broke is most of the diagnosis: a demo that never started and one that
/// ran for two full periods and then stopped give the same one-line
/// verdict and are not the same bug.
fn sample_checkers(
    n: u64,
    window: rusty_rtos_capi_core::ctypes::TickType_t,
    survived: &mut [u64; MAX_DEMOS],
    failed_at: &mut [u64; MAX_DEMOS],
) {
    for (i, demo) in DEMOS.iter().enumerate() {
        if !demos::selected(demo.name) {
            continue;
        }
        // A demo file with no checker of its own is not asked, and not
        // answered for. `flash.c` and `flash_timer.c` export a start
        // function and nothing else; a synthetic verdict here would be the
        // harness grading its own homework.
        let Some(check) = demo.check else {
            continue;
        };
        let ok = check(window) != 0;
        if let Some(v) = survived.get_mut(i) {
            // One bit per check, oldest first. The PATTERN is the
            // diagnosis: a demo whose checker latches a sticky error flag
            // never recovers, so it reads 1,0,0; one whose counter merely
            // failed to advance in a single window recovers, and reads
            // 1,0,1. Those are different bugs and the first-failure
            // number alone cannot tell them apart.
            *v |= u64::from(ok) << (n - 1);
        }
        if !ok {
            if let Some(f) = failed_at.get_mut(i) {
                if *f == 0 {
                    *f = n;
                }
            }
        }
    }
}

/// Room for the verdict table. The demo list is `const`, so this is a
/// compile-time bound and not a guess.
const MAX_DEMOS: usize = 40;

/// The monitor: the only Rust task, and the only thing that reports.
extern "C" fn monitor(_: usize) -> ! {
    // Sample through the run rather than only at the end: see SAMPLES.
    let mut survived = [0u64; MAX_DEMOS];
    let mut failed_at = [0u64; MAX_DEMOS];
    let mut last_switches = SWITCHES.load(Ordering::Relaxed);
    let slice = RUN_TICKS / SAMPLES;
    // The window every checker is asked about, in the C's own tick type.
    // `TimerDemo` judges a RATE against it; nothing else reads it.
    let window = rusty_rtos_capi_core::ctypes::TickType_t::try_from(slice)
        .unwrap_or(rusty_rtos_capi_core::ctypes::TickType_t::MAX);
    for n in 1..=SAMPLES {
        let _ = with_kernel(|k| k.delay(slice));
        yield_now();
        sample_checkers(n, window, &mut survived, &mut failed_at);
        // How much the system did between checks. A demo that stops while
        // the switch rate holds is a demo problem; all of them stopping
        // while the rate collapses is a SYSTEM problem, and the two want
        // opposite investigations.
        let sw = SWITCHES.load(Ordering::Relaxed);
        println!(
            "CAPI check {} of {}: tick {}, {} switches (+{} since the last)",
            n,
            SAMPLES,
            with_kernel(|k| k.tick_count()).unwrap_or(0),
            sw,
            sw.wrapping_sub(last_switches)
        );
        last_switches = sw;
        // The shared arenas, which twenty-one demo files draw on together.
        // A leak in any of them looks like "some demos stop after a while,
        // and which ones varies".
        println!(
            "CAPI arenas: tasks {}/{} queues {}/{} buffers {}/{} timers {}/{} heap free {}",
            abi::TASKS_MADE.load(Ordering::Relaxed),
            abi::TASKS_DELETED.load(Ordering::Relaxed),
            abi::QUEUES_MADE.load(Ordering::Relaxed),
            abi::QUEUES_DELETED.load(Ordering::Relaxed),
            abi::BUFFERS_MADE.load(Ordering::Relaxed),
            abi::BUFFERS_DELETED.load(Ordering::Relaxed),
            abi::TIMERS_MADE.load(Ordering::Relaxed),
            abi::TIMERS_DELETED.load(Ordering::Relaxed),
            abi::xPortGetFreeHeapSize()
        );
        if option_env!("KAIROS_CAPI_PROBE") == Some("checks") {
            // Every task's state at every check. Diffing check 1 against
            // check 2 says WHICH tasks stopped, which is the whole
            // question when a demo passes once and then never again.
            abi::state_line(n as u32);
        }
    }

    let switches = SWITCHES.load(Ordering::Relaxed);
    let reached = with_kernel(|k| k.tick_count()).unwrap_or(0);

    println!();
    println!("CAPI ticks asked={RUN_TICKS} reached={reached} switches={switches}");
    println!(
        "CAPI tasks made={} entered={}  queues={}  delays={}",
        abi::TASKS_MADE.load(Ordering::Relaxed),
        abi::TASKS_ENTERED.load(Ordering::Relaxed),
        abi::QUEUES_MADE.load(Ordering::Relaxed),
        abi::DELAYS.load(Ordering::Relaxed)
    );
    println!(
        "CAPI eventgroups: made={} setFromISR ok={} fail={}  getFromISR calls={} nonzero={} err={}  wait ok={} fail={}",
        abi::EG_MADE.load(Ordering::Relaxed),
        abi::EG_SETISR_OK.load(Ordering::Relaxed),
        abi::EG_SETISR_FAIL.load(Ordering::Relaxed),
        abi::EG_GETISR_CALLS.load(Ordering::Relaxed),
        abi::EG_GETISR_NONZERO.load(Ordering::Relaxed),
        abi::EG_GETISR_ERR.load(Ordering::Relaxed),
        abi::EG_WAIT_OK.load(Ordering::Relaxed),
        abi::EG_WAIT_FAIL.load(Ordering::Relaxed)
    );
    println!(
        "CAPI notify: isr calls={} delivered={}  waits={} (blocked {})  takes={} (blocked {})",
        abi::NOTIFY_ISR_CALLS.load(Ordering::Relaxed),
        abi::NOTIFY_ISR_DELIVERED.load(Ordering::Relaxed),
        abi::NOTIFY_WAITS.load(Ordering::Relaxed),
        abi::NOTIFY_WAIT_BLOCKED.load(Ordering::Relaxed),
        abi::NOTIFY_TAKES.load(Ordering::Relaxed),
        abi::NOTIFY_TAKE_BLOCKED.load(Ordering::Relaxed)
    );
    println!(
        "CAPI notify: last wait asked for {} ticks, started at tick {}",
        abi::NOTIFY_LAST_TICKS.load(Ordering::Relaxed),
        abi::NOTIFY_LAST_AT.load(Ordering::Relaxed)
    );
    println!(
        "CAPI timers: made={} started={} expiries={} callbacks run={} lost={}",
        abi::TIMERS_MADE.load(Ordering::Relaxed),
        abi::TIMERS_STARTED.load(Ordering::Relaxed),
        abi::TIMER_EXPIRIES.load(Ordering::Relaxed),
        abi::TIMER_CALLBACKS_RUN.load(Ordering::Relaxed),
        abi::EXPIRIES_LOST.load(Ordering::Relaxed)
    );
    println!(
        "CAPI tick hooks run={}  preemptive={PREEMPTIVE}  orphaned threads={}",
        TICK_HOOKS.load(Ordering::Relaxed),
        rusty_rtos_port_host::orphaned_threads()
    );
    println!();

    println!(
        "CAPI resume_all: {} calls, {} answered pdTRUE",
        abi::RESUME_ALL.load(Ordering::Relaxed),
        abi::RESUME_ALL_YIELDED.load(Ordering::Relaxed)
    );
    if option_env!("KAIROS_CAPI_PROBE") == Some("turns") {
        report_turns();
    }
    {
        let mut hist = [0u32; 10];
        for (i, slot) in hist.iter_mut().enumerate() {
            *slot = abi::SB_RECV_BYTES[i].load(Ordering::Relaxed);
        }
        println!("CAPI streambuf over-blocks began at ticks: {:?}", {
            let mut t = [0u32; 4];
            for (i, slot) in t.iter_mut().enumerate() {
                *slot = abi::SB_OVERBLOCK_AT[i].load(Ordering::Relaxed);
            }
            t
        });
        println!("CAPI streambuf trigger-test blocked TICKS: {:?}", {
            let mut t = [0u32; 10];
            for (i, slot) in t.iter_mut().enumerate() {
                *slot = abi::SB_TRIGGER_TICKS[i].load(Ordering::Relaxed);
            }
            t
        });
        println!("CAPI streambuf trigger-test receives by bytes: {:?}", {
            let mut t = [0u32; 10];
            for (i, slot) in t.iter_mut().enumerate() {
                *slot = abi::SB_TRIGGER_BYTES[i].load(Ordering::Relaxed);
            }
            t
        });
        println!(
            "CAPI streambuf: isr sends={} blocking receives={} by bytes {:?}",
            abi::SB_ISR_SENDS.load(Ordering::Relaxed),
            abi::SB_BLOCKING_RECVS.load(Ordering::Relaxed),
            hist
        );
    }
    let mut failed = 0u32;
    let mut ran = 0u32;
    let mut no_verdict = 0u32;
    if option_env!("KAIROS_CAPI_PROBE") == Some("states") {
        abi::report_tasks();
    }
    // The LED counters, which are OURS and not any demo's verdict. They
    // are the only evidence `flash.c` and `flash_timer.c` ran at all,
    // because neither file ships a checker -- so they are printed here,
    // above the verdicts, and plainly labelled.
    for led in 0..board::led_count() {
        let (toggles, sets) = board::led_activity(led);
        if toggles > 0 || sets > 0 {
            println!("CAPI led {}: toggled {} set {}", led, toggles, sets);
        }
    }
    println!("each verdict below is the DEMO's own checker, not ours:");
    for (i, demo) in DEMOS.iter().enumerate() {
        if !demos::selected(demo.name) {
            continue;
        }
        if demo.check.is_none() {
            // Ran, and there is no verdict to have. Said plainly and left
            // out of the count, because "27 of 27 demo files pass" would
            // be claiming a pass from a file that cannot give one.
            no_verdict += 1;
            continue;
        }
        ran += 1;
        if failed_at.get(i).copied().unwrap_or(1) == 0 {
            println!(
                "      ok    {:<16} still running after {SAMPLES} checks",
                demo.name
            );
        } else {
            failed += 1;
            println!(
                "      FAIL  {:<16} its checker said NO; checks {}",
                demo.name,
                pattern(survived.get(i).copied().unwrap_or(0))
            );
        }
    }
    if switches == 0 {
        failed += 1;
        println!("      FAIL  the kernel never switched -- nothing actually ran");
    } else {
        println!("      ok    the kernel switched {switches} times");
    }

    if no_verdict > 0 {
        // Not a pass and not a failure: these files have no checker to ask.
        println!(
            "      --    {no_verdict} file(s) ran with NO checker of their own; see the LED counts above"
        );
    }
    println!();
    if failed == 0 {
        println!(
            "RESULT: PASS -- {ran} unmodified C demo file(s) on the Kairos kernel, on OS threads."
        );
        std::process::exit(0);
    }
    println!("RESULT: FAIL -- {failed} check(s) failed");
    std::process::exit(1);
}

/// The idle task. Priority 0, so it runs only when nothing else is ready.
///
/// It yields rather than spinning: on a host a spinning idle task burns a
/// core for nothing, and the permit has to move for anything else to run.
extern "C" fn idle(_: usize) -> ! {
    loop {
        // `prvCheckTasksWaitingTermination`, which in the C is the idle
        // task's other job and is not optional. A task that deletes
        // ITSELF cannot free its own slot -- it is standing on it -- so
        // the kernel defers, and the idle task is what collects.
        //
        // An idle task that only yields never collects, and nothing says
        // so: the slots simply accumulate. `death.c` is the demo that
        // notices, because its checker asserts the live task count stays
        // within three of where it started, and it took a 12,000-tick run
        // to get there -- at 3,000 the count had not yet drifted past the
        // allowance. It failed identically on BOTH ports, which is what
        // said it was not a port bug.
        let _ = with_kernel(|k| k.check_tasks_waiting_termination());
        yield_now();
    }
}

/// The timer daemon, with the same body as the chip cell's.
///
/// It must SLEEP when its queue is empty rather than yield: a task that is
/// always ready starves everything below it however often it yields, and
/// yielding here is what made `PollQ` and `integer` fail on the chip.
extern "C" fn timer_daemon(_: usize) -> ! {
    loop {
        // Any callback the kernel handed us last time round, run now that
        // nothing holds the kernel.
        abi::drain_timer_expiries();

        // `prvGetNextExpireTime` + `prvProcessTimerOrBlockTask`, the half
        // that was missing. The trace-exact form is the corpus runner's
        // state machine in `rusty_rtos_demo`; this is the same sequence
        // written straight-line, which is what having a stack buys.
        let due = with_kernel(|k| {
            let (next, list_was_empty) = k.timer_next_expire();
            k.suspend_all();
            let (now, switched) = k.timer_sample_time_now().unwrap_or((0, false));
            let due = if !switched && !list_was_empty && next <= now {
                Some((next, now))
            } else {
                None
            };
            let _ = k.resume_all();
            due
        });
        if let Some(Some((at, now))) = due {
            // The callback is recorded by the hook and run at the top of
            // the next pass, outside the kernel borrow.
            let _ = with_kernel(|k| k.process_expired_timer(at, now));
            continue;
        }

        // `prvProcessTimerOrBlockTask`: wait ON the command queue, for
        // exactly as long as the next expiry allows.
        //
        // **The block time is the point, not the promptness.** A zero-wait
        // receive leaves this task off the queue's receive list — so a task
        // posting a command removes no waiter, `queue_send_generic` never
        // calls `port_yield`, and this daemon, at a HIGHER priority, still
        // does not run until something else happens to yield. Being higher
        // priority buys nothing if nobody asks for the switch.
        //
        // `TimerDemo` asserts on precisely that: it stops an auto-reload
        // timer and expects it to be inactive on the very next line,
        // because "this task is running at a priority below the timer
        // service task". With the old polling daemon and a `delay(1)`, that
        // assertion failed at `TimerDemo.c:469`, and no other demo in the
        // corpus could see it.
        let wait = with_kernel(|k| {
            let (next, list_was_empty) = k.timer_next_expire();
            if list_was_empty {
                // The C blocks indefinitely here. A long bounded wait is
                // preferred: it makes this task a real waiter, which is all
                // the yield path needs, while a wake-up this cell failed to
                // deliver degrades to a one-second delay instead of a hang.
                // In a cell whose job is to find bugs, the diagnosable
                // failure is worth more than the tidy one.
                u64::from(CapiConfig::TICK_RATE_HZ)
            } else {
                // The expiry half above already returned for anything due,
                // so this is strictly positive.
                next.saturating_sub(k.tick_count())
            }
        })
        .unwrap_or(1);
        if wait == 0 {
            abi::DAEMON_POLLED.fetch_add(1, Ordering::Relaxed);
        }

        match with_kernel(|k| k.process_one_timer_command(wait)) {
            // Did work; there may be more behind it.
            Some(Ok(Wait::Ready(true))) => {
                abi::DAEMON_WORKED.fetch_add(1, Ordering::Relaxed);
            }
            // Parked on the queue with that timeout. The kernel makes this
            // task ready again when a command arrives or the wait runs out,
            // so handing the CPU over is all that is left to do -- and
            // unlike the `delay(1)` this replaces, a command arriving one
            // tick from now is acted on THEN and not at the next tick.
            Some(Ok(Wait::Blocked)) => {
                abi::DAEMON_PARKED.fetch_add(1, Ordering::Relaxed);
                yield_now();
            }
            // Nothing came: the queue wait above ELAPSED.
            //
            // This must not sleep. The old form called `delay(1)` here,
            // and that was the whole defect `TimerDemo.c:469` found: a
            // daemon on the DELAYED list is not on the queue's receive
            // list, so a task posting a command removes no waiter,
            // `queue_send_generic` asks for no switch, and this
            // higher-priority task does not run until something else
            // happens to yield. The demo stops a timer and checks it on
            // the next line, so "something else" never comes.
            //
            // Looping instead is safe and is what `prvProcessTimerOrBlockTask`
            // does: the wait was the time to the next expiry, so its
            // elapsing means an expiry is due, and the top of the loop
            // handles that and comes back here to wait on the queue again.
            // The blocking is the QUEUE's now, which is the point.
            _ => {
                abi::DAEMON_FELL_BACK.fetch_add(1, Ordering::Relaxed);
                yield_now();
            }
        }
    }
}

// `MessageBufferAMP.c`'s stand-in for the other core's interrupt handler.
#[cfg(feature = "amp")]
unsafe extern "C" {
    fn vGenerateCoreBInterrupt(xUpdatedMessageBuffer: *mut core::ffi::c_void);
}

// The header gate's own symbol; see `capi/header_gate.c`.
unsafe extern "C" {
    fn kairos_capi_header_gate() -> core::ffi::c_ulong;
}

/// Print the kernel's own view of the world when a task thread dies.
///
/// The QEMU cell gets this for free: it has a `HardFault` handler, and
/// that handler dumps the slot table before exiting. A host has no such
/// handler -- a panic in a task thread prints a Rust backtrace and the
/// thread quietly stops, and the run then fails a great distance from the
/// cause. That is not hypothetical: the first thing this cell ever did was
/// take an access violation before any C ran, because `CEntry` held a
/// 64-bit function pointer in an `AtomicU32`, and the slot table is
/// exactly what says so.
///
/// This runs IN the panicking thread, so it must not take the kernel lock
/// -- the panicking thread may well be holding it. `report_slots` reads
/// only atomics and the port's thread table, which is why it is the thing
/// called here.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        println!("--- the kernel's view at the moment of that panic ---");
        abi::report_slots();
    }));
}

fn main() {
    install_panic_hook();
    println!();
    println!("=== K6: the UNMODIFIED C demo files on the Kairos kernel, on a HOST ===");
    println!("source  oracle/.../Demo/Common/Minimal/*.c, compiled as-is");
    println!("headers the oracle's own FreeRTOS.h / task.h / queue.h");
    println!("port    rusty_rtos_port-host: one OS thread per task, real stacks");
    // SAFETY: a C function with no arguments that reads a const array.
    let resolved = unsafe { kairos_capi_header_gate() };
    println!(
        "header  kairos_capi.h: {resolved} of {} declared symbols resolved",
        rusty_rtos_capi_core::symbols::SYMBOLS.len()
    );
    if resolved as usize != rusty_rtos_capi_core::symbols::SYMBOLS.len() {
        println!("the generated header declares symbols the seam does not define");
        std::process::exit(1);
    }
    if !PREEMPTIVE {
        // Said out loud rather than discovered from a wrong number:
        // `integer.c` never blocks and never yields, so without preemption
        // it starves every task below it and the failure reads as somebody
        // else's bug.
        println!();
        println!("WARNING: this platform's host port cannot preempt, so any demo that");
        println!("         never yields will starve the ones below it. See the port's");
        println!("         `backend` module for what is missing.");
    }
    println!();

    let kernel = match K::new(HostPort::new(), NoTrace) {
        Ok(k) => k,
        Err(e) => {
            println!("kernel refused the geometry: {e:?}");
            std::process::exit(1);
        }
    };
    // SAFETY: nothing else has been started, so no thread can be inside.
    unsafe {
        *KERNEL.0.get() = Some(kernel);
    }

    // The monitor first, so it is index 0 and outranks the demos' tasks.
    let Some(Ok(mon)) = with_kernel(|k| k.create_task("mon", 3)) else {
        println!("could not create the monitor");
        std::process::exit(1);
    };
    if !arm_task(mon, monitor) {
        println!("the monitor fell outside the slot table");
        std::process::exit(1);
    }

    for demo in DEMOS {
        if !demos::selected(demo.name) {
            continue;
        }
        (demo.start)();
        println!(
            "  started {:<16} tasks={} queues={}",
            demo.name,
            abi::TASKS_MADE.load(Ordering::Relaxed),
            abi::QUEUES_MADE.load(Ordering::Relaxed)
        );
    }

    let Some(Ok(started)) = with_kernel(|k| k.start_scheduler()) else {
        println!(
            "start_scheduler REFUSED -- {} tasks made, arena holds {TASKS}. Raise TASKS.",
            abi::TASKS_MADE.load(Ordering::Relaxed)
        );
        std::process::exit(1);
    };
    if !arm_task(started.idle, idle) || !arm_task(started.timer, timer_daemon) {
        println!("idle or timer fell outside the slot table");
        std::process::exit(1);
    }

    let Some(first) = with_kernel(|k| k.current()) else {
        println!("the kernel named no first task");
        std::process::exit(1);
    };

    set_scheduler(pick_next);
    STARTED.store(true, Ordering::SeqCst);
    // One millisecond, because `configTICK_RATE_HZ` is 1000 and
    // `CapiConfig::TICK_RATE_HZ` is 1000: `pdMS_TO_TICKS` in the C and
    // `delay` in the Rust have to mean the same duration.
    Ticker::new(Duration::from_millis(1), on_tick).spawn();

    println!(
        "starting the first task ({})...",
        usize::from(first.index())
    );
    start_first_task(usize::from(first.index()));
}
