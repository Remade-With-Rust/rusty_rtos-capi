#![no_std]
#![no_main]
//! K6 milestone 1: the **unmodified** C demo task, on the Kairos kernel.
//!
//! `oracle/FreeRTOS/FreeRTOS/Demo/Common/Minimal/PollQ.c` is compiled
//! straight out of the pinned checkout, against the real `FreeRTOS.h`,
//! `task.h` and `queue.h` from the same checkout, and linked against the
//! symbols in [`abi`]. Nothing about the C file is patched, wrapped or
//! regenerated. The only file this cell hands the C side is a
//! `FreeRTOSConfig.h`, which every FreeRTOS application supplies.
//!
//! # The inversion
//!
//! `PollQ` is already in the conformance corpus as a Rust state machine,
//! and that state machine is byte-identical to this C file's trace on four
//! architectures. So the same source is the **oracle** over there and the
//! **client** here: we proved we behave like it, and now it runs on us.
//!
//! # Why a C task can block at all
//!
//! The Kairos kernel is stackless: a blocking call returns `Wait::Blocked`,
//! meaning "call again when this task next runs". A C task cannot do that —
//! `vTaskDelay` must return *later*, with its locals intact — so a C task
//! needs a real stack and a real context switch.
//!
//! It gets one. `rusty_rtos_port-cortex-m` gives each task a stack built by
//! `init_stack`, and `PendSV` asks `Kernel::switch_context` who is next.
//! That is the same joint `mps2-an385-qemu-kernel` proves; this cell puts a
//! C function on top of it instead of a Rust one.
//!
//! # What is NOT claimed
//!
//! One demo file of thirty-four, and the eight symbols it needs. The other
//! thirty-three want a wider surface — semaphores, notifications, timers,
//! event groups — and `docs/API-MAP.md` is the list. This is milestone one,
//! not K6.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use cortex_m_rt::{entry, exception};
use cortex_m_semihosting::{debug, hprintln};
use panic_semihosting as _;

use rusty_rtos_core::config::Config;
use rusty_rtos_core::handle::TaskHandle;
use rusty_rtos_core::tick::Bits32;
use rusty_rtos_core::trace::{Event, Trace};
use rusty_rtos_kernel_core::queue::Wait;
use rusty_rtos_kernel_core::{items_for, lists_for, Kernel};
use rusty_rtos_port_cortex_m::{
    init_stack, set_scheduler, start_first_task, start_tick, CortexMPort, CURRENT_SP_SLOT,
};

/// The seam, shared with every other C ABI cell.
///
/// `#[path]` rather than a crate: see the file's own header. This cell
/// supplies the six names it asks for, below.
#[path = "../../../seam/abi.rs"]
mod abi;

/// The C side's `configTICK_RATE_HZ` and this must be the same number, or
/// `pdMS_TO_TICKS` in the demo and `delay()` in the kernel mean different
/// durations. `capi/FreeRTOSConfig.h` says 1000.
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
// Nothing here reads a task name, so the kernel is told not to build one.
    // Without this the trait default is `true` and every traced event costs a
    // name lookup plus a UTF-8 validation for a sink that drops it: measured
    // at 3.86x on one row (2026-09-21).
    const WANTS_NAMES: bool = false;

    fn event(&mut self, _tick: u64, _event: Event<'_>) {}
}

/// Sized for the demo set, not for one demo. Six files create roughly
/// twenty tasks between them, plus the kernel's idle and timer and this
/// cell's monitor. An undersized arena does not fail loudly: `xTaskCreate`
/// returns pdFAIL, the demo carries on without its tasks, and the cell
/// hangs with nothing to schedule.
pub(crate) const TASKS: usize = 96;
pub(crate) const QUEUES: usize = 48;
/// The message-slot pool, shared by every queue.
///
/// This is a TOTAL, not a per-queue depth, and getting it wrong presents as
/// the demo doing nothing at all: `PollQ` asks for a queue of ten, an
/// undersized pool makes `xQueueCreate` return NULL, and the C then skips
/// its task creation inside `if( xPolledQueue != NULL )` — silently, because
/// that is exactly what the real FreeRTOS would do too. 8 was the first
/// value here and cost a debugging session.
const SLOTS: usize = 1024;
// Four for the other demos, FOURTEEN for `TimerDemo` alone
// (`configTIMER_QUEUE_LENGTH + 1` auto-reload, one one-shot, two from the
// ISR), and room to see an over-run rather than exhaust the arena.
pub(crate) const TIMERS: usize = 24;
const GROUPS: usize = 16;
/// Stream and message buffers, and the byte pool they share. These were `1`
/// and `8` while only `PollQ` was linked -- placeholders that silently
/// refused every `xStreamBufferGenericCreate` once the buffer demos arrived.
const BUFFERS: usize = 16;
const BYTES: usize = 8192;

/// The cell's tick hook, and it exists for exactly one method.
///
/// `xEventGroupSetBitsFromISR` cannot walk an event group's waiting list
/// from an interrupt, so the kernel defers it to the daemon task through
/// `xTimerPendFunctionCallFromISR` -- and the daemon then asks the
/// application's [`TickHook::pended`] what to do with it. With
/// `NoTickHook` that method is a no-op, so the deferral was accepted,
/// counted as `pdPASS`, and dropped: `EventGroupsDemo` set bits from the
/// tick hook ten times and read `0x00` back every time, because nothing
/// ever performed the set.
///
/// The two event-group deferrals are the kernel's own work, so they go
/// straight back to it -- the same routing `rusty_rtos_demo`'s runner does
/// for the conformance corpus.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CapiTickHook;

impl rusty_rtos_core::hooks::TickHook<K> for CapiTickHook {
    fn tick(self, _kernel: &mut K) -> Self {
        // The demo ISR halves run from this cell's `SysTick` handler,
        // which is the point in the C where `vApplicationTickHook` is
        // called. Nothing extra belongs here.
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
    CortexMPort,
    NoTrace,
    CapiTickHook,
    TASKS,
    { list_slots_for(TASKS, TIMERS, lists_for(CapiConfig::MAX_PRIORITIES, QUEUES, GROUPS)) },
    { lists_for(CapiConfig::MAX_PRIORITIES, QUEUES, GROUPS) },
    QUEUES,
    SLOTS,
    BUFFERS,
    BYTES,
    TIMERS,
    GROUPS,
>;

/// The kernel, reachable from a task and from `PendSV`.
pub(crate) struct KernelCell(UnsafeCell<Option<K>>);
// SAFETY: every access goes through `with_kernel`, which masks interrupts,
// or through `PendSV`, the lowest-priority exception, which therefore runs
// with no task on the CPU. One core.
unsafe impl Sync for KernelCell {}
pub(crate) static KERNEL: KernelCell = KernelCell(UnsafeCell::new(None));

// ------------------------------- the four verbs the shared seam asks for --
//
// `seam/abi.rs` is written against these and names no chip. On this cell
// they are the Cortex-M3's; on `hosted/capi-host` they are the host's.

/// `yield_now`: leave the CPU and come back when the kernel says so.
pub(crate) fn yield_now() {
    rusty_rtos_port_cortex_m::pend_switch();
}

/// `without_interrupts`: run `f` with nothing able to preempt it.
pub(crate) fn without_interrupts<R>(f: impl FnOnce() -> R) -> R {
    cortex_m::interrupt::free(|_| f())
}

/// `note`: say something to whoever is watching.
///
/// A macro rather than a function taking `Arguments`, because the seam
/// calls it the way it calls `hprintln!` and a macro keeps the call sites
/// identical across cells.
#[macro_export]
macro_rules! note {
    ($($arg:tt)*) => {
        ::cortex_m_semihosting::hprintln!($($arg)*)
    };
}

/// `die`: stop, with a failing status.
pub(crate) fn die() -> ! {
    debug::exit(debug::EXIT_FAILURE);
    loop {
        core::hint::spin_loop();
    }
}

/// `where_it_runs`: where task `i` will resume, or 0 if nowhere yet.
///
/// On this cell that is its saved stack pointer.
pub(crate) fn where_it_runs(i: usize) -> usize {
    SLOTS_SP.get(i).map_or(0, |s| s.load(Ordering::Relaxed))
}

/// Borrow the kernel with interrupts masked.
pub(crate) fn with_kernel<R>(f: impl FnOnce(&mut K) -> R) -> Option<R> {
    cortex_m::interrupt::free(|_| {
        // SAFETY: interrupts are masked, so neither SysTick nor PendSV can
        // be holding this, and there is no second core.
        let slot = unsafe { &mut *KERNEL.0.get() };
        slot.as_mut().map(f)
    })
}

/// As `with_kernel`, but for an exception, which already has exclusivity.
fn with_kernel_in_exception<R>(f: impl FnOnce(&mut K) -> R) -> Option<R> {
    // SAFETY: PendSV and SysTick cannot preempt one another here, and no
    // task runs while an exception is active.
    let slot = unsafe { &mut *KERNEL.0.get() };
    slot.as_mut().map(f)
}

/// Words per task stack. `configMINIMAL_STACK_SIZE` on the C side is 256;
/// the C task's `usStackDepth` argument is honoured up to this ceiling and
/// [`abi::x_task_create`] refuses anything larger rather than overflowing.
pub(crate) const STACK_WORDS: usize = 512;

/// One stack per task slot. Static because there is no heap: a C program
/// asking for a task gets one of these, or an honest failure.
pub(crate) static mut STACKS: [[usize; STACK_WORDS]; TASKS] = [[0; STACK_WORDS]; TASKS];

/// One saved stack pointer per task slot, indexed by `TaskHandle::index`.
/// This is what `CURRENT_SP_SLOT` points into.
#[allow(clippy::declare_interior_mutable_const)]
const NO_SP: AtomicUsize = AtomicUsize::new(0);
pub(crate) static SLOTS_SP: [AtomicUsize; TASKS] = [NO_SP; TASKS];

static SWITCHES: AtomicU32 = AtomicU32::new(0);

static PORT: CortexMPort = CortexMPort::new();

/// `PendSV` asks the kernel who is next and points the port at that task's
/// saved-SP slot. The whole joint, unchanged from the sibling cell.
extern "C" fn pick_next() {
    let next = with_kernel_in_exception(|k| {
        k.switch_context();
        k.current()
    });
    if let Some(handle) = next {
        let i = usize::from(handle.index());
        if let Some(slot) = SLOTS_SP.get(i) {
            // A saved stack pointer must lie inside its OWN task's stack.
            // `PendSV` writes the outgoing task's SP through whatever
            // `CURRENT_SP_SLOT` points at and reads the incoming one back
            // the same way, so a slot holding a pointer into somebody
            // else's stack means two tasks are about to share one, and the
            // symptom arrives much later as a fault with no PC worth
            // reading. One compare here names it at the moment it happens.
            // Zero is not exempt: it is the WORST value. `PendSV` skips
            // its restore when the incoming slot reads zero, so the
            // outgoing task simply keeps running -- on its own stack, but
            // with `CURRENT_SP_SLOT` now pointing at the incoming task's
            // slot. The next switch then writes one task's stack pointer
            // into another task's slot and the two share a stack.
            let sp = slot.load(Ordering::Relaxed);
            if !sp_is_in_stack(i, sp) {
                hprintln!(
                    "CAPI SLOT CORRUPT: task {} saved sp={:#010x}, its stack is {:#010x}..{:#010x}",
                    i,
                    sp,
                    stack_lo(i),
                    stack_lo(i) + STACK_WORDS * 4
                );
                let who = with_kernel_in_exception(|k| {
                    (k.name_of(handle), k.state_of(handle), k.task_count())
                });
                hprintln!("CAPI SLOT CORRUPT: the kernel calls it {:?}", who);
                debug::exit(debug::EXIT_FAILURE);
            }
            CURRENT_SP_SLOT.store(core::ptr::from_ref(slot) as usize, Ordering::Relaxed);
            SWITCHES.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// The lowest address of task `i`'s stack.
pub(crate) fn stack_lo(i: usize) -> usize {
    // SAFETY: `&raw` only; no reference to the `static mut` is formed.
    (core::ptr::addr_of!(STACKS) as usize) + i * STACK_WORDS * core::mem::size_of::<usize>()
}

/// Is `sp` a plausible saved stack pointer for task `i`?
pub(crate) fn sp_is_in_stack(i: usize, sp: usize) -> bool {
    let lo = stack_lo(i);
    sp >= lo && sp <= lo + STACK_WORDS * core::mem::size_of::<usize>()
}

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

// The header gate's own symbol; see `capi/header_gate.c`.
unsafe extern "C" {
    fn kairos_capi_header_gate() -> core::ffi::c_ulong;
}

/// A fault has no handler by default, and `cortex-m-rt`'s fallback is an
/// infinite loop inside an exception that outranks `SysTick` -- so a faulting
/// C task presents as the whole system stopping, with no message, which is
/// indistinguishable from a deadlock or a starved monitor. It cost a session
/// to tell those apart once; it costs eight lines to never have to again.
///
/// The stacked frame names the instruction, and on Cortex-M3 the fault
/// status registers name the reason.
#[exception]
unsafe fn HardFault(ef: &cortex_m_rt::ExceptionFrame) -> ! {
    // SAFETY: the SCB fault status registers are readable at any privilege
    // level this firmware runs at, and we are already dead.
    let cfsr = unsafe { core::ptr::read_volatile(0xE000_ED28 as *const u32) };
    let hfsr = unsafe { core::ptr::read_volatile(0xE000_ED2C as *const u32) };
    let mmar = unsafe { core::ptr::read_volatile(0xE000_ED34 as *const u32) };
    let bfar = unsafe { core::ptr::read_volatile(0xE000_ED38 as *const u32) };
    hprintln!(
        "CAPI HARDFAULT pc={:#010x} lr={:#010x} r0={:#010x} r1={:#010x} r2={:#010x} r3={:#010x}",
        ef.pc(),
        ef.lr(),
        ef.r0(),
        ef.r1(),
        ef.r2(),
        ef.r3()
    );
    hprintln!(
        "CAPI HARDFAULT cfsr={:#010x} hfsr={:#010x} mmar={:#010x} bfar={:#010x} current={:?}",
        cfsr,
        hfsr,
        mmar,
        bfar,
        with_kernel_in_exception(|k| k.current()).map(|t| usize::from(t.index()))
    );
    // The stack above the frame still holds the return addresses that led
    // here. A fault that branched to 0 has no PC worth reading, so the only
    // way to name the caller is to dump the words and look them up.
    let sp = core::ptr::from_ref(ef) as usize;
    hprintln!("CAPI HARDFAULT sp={:#010x}, stack above the frame:", sp);
    for row in 0..6 {
        let mut line = [0usize; 4];
        for col in 0..4 {
            // SAFETY: reading words of the faulting task's own stack.
            line[col] =
                unsafe { ((sp + 32 + (row * 4 + col) * 4) as *const usize).read_volatile() };
        }
        hprintln!(
            "    +{:<3} {:#010x} {:#010x} {:#010x} {:#010x}",
            32 + row * 16,
            line[0],
            line[1],
            line[2],
            line[3]
        );
    }
    report_stacks();
    abi::report_slots();
    hprintln!(
        "CAPI current_sp_slot={:#010x}",
        CURRENT_SP_SLOT.load(Ordering::Relaxed)
    );
    debug::exit(debug::EXIT_FAILURE);
    loop {
        core::hint::spin_loop();
    }
}

#[exception]
fn SysTick() {
    SYSTICKS.fetch_add(1, Ordering::Relaxed);
    let want = with_kernel_in_exception(|k| k.increment_tick()).unwrap_or(false);
    if HEARTBEAT.is_some() {
        let t = with_kernel_in_exception(|k| k.tick_count()).unwrap_or(0);
        TICKS_SEEN.store(t as u32, Ordering::Relaxed);
    }

    // Only once the scheduler is running: these touch queues and
    // semaphores the demos create, and calling them before
    // `vTaskStartScheduler` would reach objects that do not exist yet.
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

    // A hang looks identical to a deadlock, a starved monitor and a fault
    // loop from outside. `KAIROS_CAPI_HEARTBEAT=250` makes the tick say, out
    // loud, whether time is advancing and WHO is running while it does --
    // which is the difference between those three in one run.
    if let Some(every) = HEARTBEAT {
        let n = SYSTICKS.load(Ordering::Relaxed);
        if n % every == 0 {
            let who = with_kernel_in_exception(|k| k.current())
                .map(|t| usize::from(t.index()))
                .unwrap_or(usize::MAX);
            hprintln!(
                "  .. systick={} kernel_tick={} current={}",
                n,
                TICKS_SEEN.load(Ordering::Relaxed),
                who
            );
        }
    }

    // `portYIELD_FROM_ISR( xHigherPriorityTaskWoken )`, for the callers
    // that passed NULL. `increment_tick` was asked BEFORE the ISR halves
    // ran, so a task one of them woke is not in `want` -- and a wake-up
    // dropped here is a task that stays ready and unscheduled until
    // something else happens to yield.
    let want = want || abi::take_isr_woke();

    rusty_rtos_port_cortex_m::tick(&PORT, want);
}

/// Print a heartbeat every N SysTicks. Off unless the env var is set at
/// build time, so it costs nothing in a normal run.
const HEARTBEAT: Option<u32> = match option_env!("KAIROS_CAPI_HEARTBEAT") {
    Some(s) => match u32::from_str_radix(s, 10) {
        Ok(n) if n > 0 => Some(n),
        _ => None,
    },
    None => None,
};

/// The kernel's own tick, sampled in the handler so the heartbeat can show
/// it without a second kernel borrow.
static TICKS_SEEN: AtomicU32 = AtomicU32::new(0);

/// Set once the scheduler is running, so the tick hook knows the demos'
/// objects exist.
static STARTED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// How many times the tick hook ran its demo ISR halves.
static TICK_HOOKS: AtomicU32 = AtomicU32::new(0);
/// How many times `SysTick` fired at all.
static SYSTICKS: AtomicU32 = AtomicU32::new(0);

/// Give one task a stack and record it in its own slot.
pub(crate) fn arm_task(handle: TaskHandle, entry: extern "C" fn(usize) -> !) -> bool {
    let i = usize::from(handle.index());
    let Some(slot) = SLOTS_SP.get(i) else {
        return false;
    };
    // SAFETY: `&raw mut` so no reference to a `static mut` is formed, and
    // `i` is inside TASKS because the slot lookup above succeeded.
    let top = unsafe {
        core::ptr::addr_of_mut!(STACKS)
            .cast::<usize>()
            .add(i * STACK_WORDS + STACK_WORDS)
    };
    // Paint the whole stack before the frame goes on it, so the depth a
    // task actually reached can be read back afterwards. `configASSERT` and
    // `uxTaskGetStackHighWaterMark` are the C's answers to this question and
    // neither is available here: the config turns the high-water mark off,
    // and an overflow does not trip an assert -- it silently writes into the
    // NEXT task's stack, and the two tasks then fail in ways that have
    // nothing to do with each other.
    //
    // SAFETY: same bounds as `top` below; `i < TASKS`.
    unsafe {
        let base = core::ptr::addr_of_mut!(STACKS)
            .cast::<usize>()
            .add(i * STACK_WORDS);
        for w in 0..STACK_WORDS {
            base.add(w).write(STACK_PAINT);
        }
    }
    slot.store(init_stack(top, entry, i), Ordering::SeqCst);
    true
}

/// The pattern an untouched stack word holds. Arbitrary, and deliberately
/// not a plausible pointer or a small integer, so a word that still reads
/// as this one has genuinely never been written.
pub(crate) const STACK_PAINT: usize = 0xA5A5_5A5A;

/// How deep task `i` has been, in words, and whether it went off the end.
///
/// Returns `(used, overflowed)`. `overflowed` means the BOTTOM word of the
/// stack has been written, at which point `used` is a floor, not a
/// measurement -- the task has already been writing into its neighbour.
pub(crate) fn stack_used(i: usize) -> (usize, bool) {
    if i >= TASKS {
        return (0, false);
    }
    // SAFETY: `i < TASKS`, so the whole row is in bounds.
    let base = unsafe {
        core::ptr::addr_of_mut!(STACKS)
            .cast::<usize>()
            .add(i * STACK_WORDS)
    };
    let mut untouched = 0usize;
    while untouched < STACK_WORDS {
        // SAFETY: bounded by STACK_WORDS.
        if unsafe { base.add(untouched).read_volatile() } != STACK_PAINT {
            break;
        }
        untouched += 1;
    }
    (STACK_WORDS - untouched, untouched == 0)
}

/// Report every armed task's stack depth, and say which ones overflowed.
pub(crate) fn report_stacks() {
    let mut worst = 0usize;
    let mut over = 0usize;
    for i in 0..TASKS {
        if SLOTS_SP[i].load(Ordering::Relaxed) == 0 {
            continue;
        }
        let (used, of) = stack_used(i);
        if used > worst {
            worst = used;
        }
        if of {
            over += 1;
            hprintln!("      OVERFLOW task {} used all {} words", i, STACK_WORDS);
        }
    }
    hprintln!(
        "CAPI stacks: deepest {} of {} words ({} bytes of {}), overflows={}",
        worst,
        STACK_WORDS,
        worst * 4,
        STACK_WORDS * 4,
        over
    );
}

// ---------------------------------------------------------------- the C --

/// How long the C demo runs before it is asked whether it is still healthy.
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

/// Room for the verdict table. `DEMOS` is `const`, so this is a
/// compile-time bound rather than a guess.
const MAX_DEMOS: usize = 40;

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

/// The monitor: the only Rust task, and the only thing that reports.
///
/// It sits above the demo's own tasks so that its delay expiring preempts
/// them, and asks `PollQ.c` its own question — `xArePollingQueuesStillRunning`
/// is the check the C demo ships with, so the verdict is the demo's, not
/// ours.
extern "C" fn monitor(_: usize) -> ! {
    // Sample through the run rather than only at the end; see SAMPLES.
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
        rusty_rtos_port_cortex_m::pend_switch();
        sample_checkers(n, window, &mut survived, &mut failed_at);
        // How much the system did between checks. A demo that stops while
        // the switch rate holds is a demo problem; all of them stopping
        // while the rate collapses is a SYSTEM problem, and the two want
        // opposite investigations.
        let sw = SWITCHES.load(Ordering::Relaxed);
        hprintln!(
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
        hprintln!(
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

    hprintln!();
    // Print the tick the KERNEL actually reached, not the constant we asked
    // to sleep for. They are not the same number, and printing the constant
    // hid that the tick hook had run 59 times against an expected 3,000.
    let reached = with_kernel(|k| k.tick_count()).unwrap_or(0);
    hprintln!(
        "CAPI ticks asked={} reached={} switches={}",
        RUN_TICKS,
        reached,
        switches
    );
    hprintln!(
        "CAPI tasks made={} entered={}  queues={}  delays={}",
        abi::TASKS_MADE.load(Ordering::Relaxed),
        abi::TASKS_ENTERED.load(Ordering::Relaxed),
        abi::QUEUES_MADE.load(Ordering::Relaxed),
        abi::DELAYS.load(Ordering::Relaxed)
    );
    hprintln!(
        "CAPI eventgroups made={} setFromISR ok={} fail={}  wait ok={} fail={}",
        abi::EG_MADE.load(Ordering::Relaxed),
        abi::EG_SETISR_OK.load(Ordering::Relaxed),
        abi::EG_SETISR_FAIL.load(Ordering::Relaxed),
        abi::EG_WAIT_OK.load(Ordering::Relaxed),
        abi::EG_WAIT_FAIL.load(Ordering::Relaxed)
    );
    hprintln!(
        "CAPI SysTick fired={} tick hooks run={}",
        SYSTICKS.load(Ordering::Relaxed),
        TICK_HOOKS.load(Ordering::Relaxed)
    );
    hprintln!(
        "CAPI eg getFromISR calls={} nonzero={} err={}",
        abi::EG_GETISR_CALLS.load(Ordering::Relaxed),
        abi::EG_GETISR_NONZERO.load(Ordering::Relaxed),
        abi::EG_GETISR_ERR.load(Ordering::Relaxed)
    );
    hprintln!(
        "CAPI semGiveFromISR ok={} fail={}",
        abi::SEM_GIVE_ISR_OK.load(Ordering::Relaxed),
        abi::SEM_GIVE_ISR_FAIL.load(Ordering::Relaxed)
    );
    hprintln!(
        "CAPI sends ok={} fail={}   receives ok={} fail={}",
        abi::SENDS_OK.load(Ordering::Relaxed),
        abi::SENDS_FAIL.load(Ordering::Relaxed),
        abi::RECVS_OK.load(Ordering::Relaxed),
        abi::RECVS_FAIL.load(Ordering::Relaxed)
    );
    hprintln!();

    report_stacks();

    hprintln!(
        "CAPI resume_all: {} calls, {} answered pdTRUE",
        abi::RESUME_ALL.load(Ordering::Relaxed),
        abi::RESUME_ALL_YIELDED.load(Ordering::Relaxed)
    );
    {
        let mut hist = [0u32; 10];
        for (i, slot) in hist.iter_mut().enumerate() {
            *slot = abi::SB_RECV_BYTES[i].load(Ordering::Relaxed);
        }
        hprintln!("CAPI streambuf over-blocks began at ticks: {:?}", {
            let mut t = [0u32; 4];
            for (i, slot) in t.iter_mut().enumerate() {
                *slot = abi::SB_OVERBLOCK_AT[i].load(Ordering::Relaxed);
            }
            t
        });
        hprintln!("CAPI streambuf trigger-test blocked TICKS: {:?}", {
            let mut t = [0u32; 10];
            for (i, slot) in t.iter_mut().enumerate() {
                *slot = abi::SB_TRIGGER_TICKS[i].load(Ordering::Relaxed);
            }
            t
        });
        hprintln!("CAPI streambuf trigger-test receives by bytes: {:?}", {
            let mut t = [0u32; 10];
            for (i, slot) in t.iter_mut().enumerate() {
                *slot = abi::SB_TRIGGER_BYTES[i].load(Ordering::Relaxed);
            }
            t
        });
        hprintln!(
            "CAPI streambuf: isr sends={} blocking receives={} by bytes {:?}",
            abi::SB_ISR_SENDS.load(Ordering::Relaxed),
            abi::SB_BLOCKING_RECVS.load(Ordering::Relaxed),
            hist
        );
    }
    let mut failed = 0u32;
    // The LED counters, which are OURS and not any demo's verdict. They
    // are the only evidence `flash.c` and `flash_timer.c` ran at all,
    // because neither file ships a checker -- so they are printed here,
    // above the verdicts, and plainly labelled.
    for led in 0..board::led_count() {
        let (toggles, sets) = board::led_activity(led);
        if toggles > 0 || sets > 0 {
            hprintln!("CAPI led {}: toggled {} set {}", led, toggles, sets);
        }
    }
    hprintln!("each verdict below is the DEMO's own checker, not ours:");
    let mut ran = 0u32;
    let mut no_verdict = 0u32;
    for (i, demo) in DEMOS.iter().enumerate() {
        if !demos::selected(demo.name) {
            continue;
        }
        if demo.check.is_none() {
            // Ran, and there is no verdict to have. Said plainly and left
            // out of the count, because "27 of 27 demo files pass" would be
            // claiming a pass from a file that cannot give one.
            no_verdict += 1;
            continue;
        }
        ran += 1;
        if failed_at.get(i).copied().unwrap_or(1) == 0 {
            hprintln!(
                "      ok    {:<10} still running after {} checks",
                demo.name,
                SAMPLES
            );
        } else {
            failed += 1;
            hprintln!(
                "      FAIL  {:<10} its checker said NO; checks {}",
                demo.name,
                pattern(survived.get(i).copied().unwrap_or(0))
            );
        }
    }
    if switches == 0 {
        failed += 1;
        hprintln!("      FAIL  the kernel never switched -- nothing actually ran");
    } else {
        hprintln!("      ok    the kernel switched {} times", switches);
    }

    if no_verdict > 0 {
        // Not a pass and not a failure: these files have no checker to ask.
        hprintln!(
            "      --    {} file(s) ran with NO checker of their own; see the LED counts above",
            no_verdict
        );
    }
    hprintln!();
    if failed == 0 {
        hprintln!(
            "RESULT: PASS -- {} unmodified C demo file(s) on the Kairos kernel.",
            ran
        );
        debug::exit(debug::EXIT_SUCCESS);
    } else {
        hprintln!("RESULT: FAIL -- {} check(s) failed", failed);
        debug::exit(debug::EXIT_FAILURE);
    }
    loop {
        core::hint::spin_loop();
    }
}

/// The idle task. Priority 0, so it runs only when nothing else is ready.
///
/// It must **not** call `idle_hook_tick`, and that was a real bug here. That
/// function is the SIM port's tick source — its own doc calls it "a critical
/// section around one unconditional tick" — and on a port with a live
/// SysTick it double-drives time. Copied from the sibling cell, it delivered
/// **2,955 of 3,001 ticks**: simulated time raced ahead of wall time, the run
/// ended after only 46 SysTicks, and every demo's ISR half therefore ran 46
/// times instead of 3,000. `EventGroupsDemo` needs its ISR half called 300
/// times before it does anything at all, so it simply never started.
///
/// On a real port the tick has exactly one source, and it is the timer.
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
        rusty_rtos_port_cortex_m::pend_switch();
    }
}

/// The timer daemon, with a real body.
///
/// It was parked in a spin loop first, which starved every task below its
/// priority — of nineteen tasks across six demo files only the two at its
/// own priority ran. Suspending it fixed that and broke something else:
/// **`xEventGroupSetBitsFromISR` defers its work to this daemon**, so with
/// the daemon asleep the bits were never set and `EventGroupsDemo`'s
/// checker reported failure. `xTimerPendFunctionCall` has the same shape.
///
/// So it gets what the C gives it: a loop over its command queue.
/// `process_one_timer_command` does the work of `prvProcessReceivedCommands`
/// one command at a time — the C's loop unrolled, because a callback in the
/// middle of it can block and the daemon has to be able to come back — and
/// parks the task when the queue is empty, which is what stops it starving
/// anything.
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
            // Looping instead is safe and is what
            // `prvProcessTimerOrBlockTask` does: the wait was the time to
            // the next expiry, so its elapsing means an expiry is due, and
            // the top of the loop handles that and comes back here to wait
            // on the queue again. The blocking is the QUEUE's now.
            _ => {
                abi::DAEMON_FELL_BACK.fetch_add(1, Ordering::Relaxed);
                rusty_rtos_port_cortex_m::pend_switch();
            }
        }
    }
}

#[entry]
fn main() -> ! {
    hprintln!();
    hprintln!("=== K6: the UNMODIFIED C demo task on the Kairos kernel ===");
    hprintln!("source  oracle/.../Demo/Common/Minimal/PollQ.c, compiled as-is");
    hprintln!("headers the oracle's own FreeRTOS.h / task.h / queue.h");
    hprintln!("symbols supplied by this cell's `abi` module");
    // The header gate, reported rather than assumed. `kairos_capi.h` is
    // generated from the crate's symbol table and compiled in the same
    // translation unit as the oracle's real headers, so a declaration that
    // disagrees with FreeRTOS is a compile error -- but the COMPLETENESS
    // half only means something if the address table survives the linker,
    // and it only survives if something calls this.
    //
    // SAFETY: a C function with no arguments that reads a const array.
    let resolved = unsafe { kairos_capi_header_gate() };
    hprintln!(
        "header  kairos_capi.h: {} of {} declared symbols resolved",
        resolved,
        rusty_rtos_capi_core::symbols::SYMBOLS.len()
    );
    if resolved as usize != rusty_rtos_capi_core::symbols::SYMBOLS.len() {
        hprintln!("the generated header declares symbols the seam does not define");
        debug::exit(debug::EXIT_FAILURE);
    }
    hprintln!();

    let kernel = match K::new(CortexMPort::new(), NoTrace) {
        Ok(k) => k,
        Err(e) => {
            hprintln!("kernel refused the geometry: {:?}", e);
            debug::exit(debug::EXIT_FAILURE);
            loop {
                core::hint::spin_loop();
            }
        }
    };
    // SAFETY: nothing else holds the kernel yet; interrupts are masked from
    // reset until `start_first_task` enables them.
    cortex_m::interrupt::free(|_| unsafe {
        *KERNEL.0.get() = Some(kernel);
    });

    // The monitor first, so it is index 0 and outranks the demo's tasks.
    let mon = with_kernel(|k| k.create_task("mon", 3))
        .and_then(Result::ok)
        .expect("monitor task");
    if !arm_task(mon, monitor) {
        hprintln!("the monitor fell outside the slot table");
        debug::exit(debug::EXIT_FAILURE);
    }

    if option_env!("KAIROS_CAPI_PROBE") == Some("queueset") {
        probe_queueset();
    }

    // Hand over to the C. This calls `xQueueCreate` once and `xTaskCreate`
    // twice, through `abi`, and every one of those reaches the kernel.
    // SAFETY: the kernel is installed, so the ABI has somewhere to go.
    for demo in DEMOS {
        if !demos::selected(demo.name) {
            continue;
        }
        (demo.start)();
        hprintln!(
            "  started {:<10} tasks={} queues={}",
            demo.name,
            abi::TASKS_MADE.load(Ordering::Relaxed),
            abi::QUEUES_MADE.load(Ordering::Relaxed)
        );
    }

    let Some(Ok(started)) = with_kernel(|k| k.start_scheduler()) else {
        // This is what a full task arena looks like from here: every demo
        // reported "started", the arena saturated part-way through, and the
        // kernel then had no room for its own idle and timer tasks.
        hprintln!(
            "start_scheduler REFUSED -- {} tasks made, arena holds {}. Raise TASKS.",
            abi::TASKS_MADE.load(Ordering::Relaxed),
            TASKS
        );
        debug::exit(debug::EXIT_FAILURE);
        loop {
            core::hint::spin_loop();
        }
    };
    if !arm_task(started.idle, idle) || !arm_task(started.timer, timer_daemon) {
        hprintln!("idle or timer fell outside the slot table");
        debug::exit(debug::EXIT_FAILURE);
    }

    let first = with_kernel(|k| k.current()).expect("a current task");
    let i = usize::from(first.index());
    if let Some(slot) = SLOTS_SP.get(i) {
        CURRENT_SP_SLOT.store(core::ptr::from_ref(slot) as usize, Ordering::SeqCst);
    }

    set_scheduler(pick_next);
    STARTED.store(true, Ordering::SeqCst);
    // 80,000 cycles per tick, which is exactly `configCPU_CLOCK_HZ` /
    // `configTICK_RATE_HZ`. These two ends of one number live in different
    // languages and nothing checks them against each other, so they are
    // written down together: the config's comment carries the measurement
    // that chose 80 MHz, and this is the SysTick half of it.
    start_tick(80_000);

    hprintln!("starting the first task ({})...", i);
    // SAFETY: every task the kernel knows about has a stack, a scheduler is
    // installed, and CURRENT_SP_SLOT names the current task's slot.
    unsafe { start_first_task() }
}

/// Replay `QueueSet.c`'s `prvSetupTest` sequence, one call at a time.
///
/// `prvSetupTest` funnels a dozen distinct checks into a single
/// `configASSERT( xQueueSetTasksStatus != pdFAIL )` at `QueueSet.c:1138`,
/// so the C tells us that SOMETHING in the set API is wrong and nothing
/// about which thing. `xQueueSetTasksStatus` is a file-scope static, so it
/// cannot be read from here either.
///
/// This walks the same calls in the same order and prints each answer
/// against the one the C requires, which turns one assertion into a line
/// number. It runs only under `KAIROS_CAPI_PROBE=queueset`.
fn probe_queueset() {
    use abi::*;
    const N: usize = 3;
    const LEN: usize = 3;

    let pass = |b: BaseType_t| if b == 1 { "PASS" } else { "FAIL" };
    let check = |what: &str, got: BaseType_t, want: BaseType_t| {
        hprintln!(
            "  probe {:<46} got {} want {}  {}",
            what,
            pass(got),
            pass(want),
            if got == want { "ok" } else { "<-- WRONG" }
        );
    };

    let set = xQueueCreateSet((N * LEN) as UBaseType_t);
    hprintln!("  probe xQueueCreateSet({}) -> {:?}", N * LEN, set);
    let mut qs = [core::ptr::null_mut(); N];
    for (x, q) in qs.iter_mut().enumerate() {
        *q = xQueueGenericCreate(LEN as UBaseType_t, 4, 0);
        hprintln!("  probe xQueueCreate #{} -> {:?}", x, *q);
        check("xQueueAddToSet(fresh queue)", xQueueAddToSet(*q, set), 1);
        check(
            "xQueueAddToSet(same queue again)",
            xQueueAddToSet(*q, set),
            0,
        );
    }
    check(
        "xQueueRemoveFromSet(q0, NULL)",
        xQueueRemoveFromSet(qs[0], core::ptr::null_mut()),
        0,
    );
    check(
        "xQueueRemoveFromSet(q0, its own set)",
        xQueueRemoveFromSet(qs[0], set),
        1,
    );

    let v: u32 = 0;
    // SAFETY: `&v` is four readable bytes, which is the queue's item size.
    let sent = unsafe { xQueueGenericSend(qs[0], core::ptr::from_ref(&v).cast(), 0, 0) };
    hprintln!("  probe xQueueSend(q0) -> {}", pass(sent));
    check(
        "xQueueAddToSet(NON-EMPTY queue)",
        xQueueAddToSet(qs[0], set),
        0,
    );

    let mut out: u32 = 0;
    // SAFETY: `&mut out` is four writable bytes.
    let got = unsafe { xQueueReceive(qs[0], core::ptr::from_mut(&mut out).cast(), 0) };
    hprintln!("  probe xQueueReceive(q0) -> {} value {}", pass(got), out);
    check(
        "xQueueAddToSet(emptied queue)",
        xQueueAddToSet(qs[0], set),
        1,
    );

    let sel = xQueueSelectFromSet(set, 200);
    hprintln!(
        "  probe xQueueSelectFromSet(set, 200) -> {:?}  want NULL  {}",
        sel,
        if sel.is_null() { "ok" } else { "<-- WRONG" }
    );

    // prvTestQueueOverwriteWithQueueSet: a length-one queue in the set,
    // overwritten twice. The second overwrite must NOT add a second entry
    // to the set -- the number of items in the member queues did not
    // change, so neither may the set's.
    hprintln!("  probe -- prvTestQueueOverwriteWithQueueSet");
    let n = |what: &str, got: UBaseType_t, want: UBaseType_t| {
        hprintln!(
            "  probe {:<46} got {} want {}  {}",
            what,
            got,
            want,
            if got == want { "ok" } else { "<-- WRONG" }
        );
    };
    let q1 = xQueueGenericCreate(1, 4, 0);
    check("xQueueAddToSet(len-1 queue)", xQueueAddToSet(q1, set), 1);
    let mut v1: u32 = 0;
    // SAFETY: four readable bytes, the queue's item size. Position 2 is
    // `queueOVERWRITE`, which is what the `xQueueOverwrite` macro passes.
    let ow = unsafe { xQueueGenericSend(q1, core::ptr::from_ref(&v1).cast(), 0, 2) };
    hprintln!("  probe xQueueOverwrite #1 -> {}", pass(ow));
    n(
        "uxQueueMessagesWaiting(set) after 1 overwrite",
        uxQueueMessagesWaiting(set),
        1,
    );
    let mut peeked: *mut core::ffi::c_void = core::ptr::null_mut();
    // SAFETY: a handle-sized destination, which is what a set yields.
    let pk = unsafe { xQueuePeek(set, core::ptr::from_mut(&mut peeked).cast(), 0) };
    hprintln!(
        "  probe xQueuePeek(set) -> {} handle {:?} want {:?}  {}",
        pass(pk),
        peeked,
        q1,
        if peeked == q1 { "ok" } else { "<-- WRONG" }
    );
    v1 += 1;
    // SAFETY: as above.
    let ow2 = unsafe { xQueueGenericSend(q1, core::ptr::from_ref(&v1).cast(), 0, 2) };
    hprintln!("  probe xQueueOverwrite #2 -> {}", pass(ow2));
    n(
        "uxQueueMessagesWaiting(set) after 2 overwrites",
        uxQueueMessagesWaiting(set),
        1,
    );
    let sel2 = xQueueSelectFromSet(set, 0);
    hprintln!(
        "  probe xQueueSelectFromSet(set) -> {:?} want {:?}  {}",
        sel2,
        q1,
        if sel2 == q1 { "ok" } else { "<-- WRONG" }
    );
    let mut got1: u32 = 0;
    // SAFETY: four writable bytes.
    let rc = unsafe { xQueueReceive(q1, core::ptr::from_mut(&mut got1).cast(), 0) };
    hprintln!(
        "  probe xQueueReceive(q1) -> {} value {} want {}  {}",
        pass(rc),
        got1,
        v1,
        if got1 == v1 { "ok" } else { "<-- WRONG" }
    );
    n(
        "uxQueueMessagesWaiting(set) when drained",
        uxQueueMessagesWaiting(set),
        0,
    );
    let sel3 = xQueueSelectFromSet(set, 0);
    hprintln!(
        "  probe xQueueSelectFromSet(drained set) -> {:?} want NULL  {}",
        sel3,
        if sel3.is_null() { "ok" } else { "<-- WRONG" }
    );
}
