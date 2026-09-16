// The C's names are the C's. `xQueueGenericSend` is the symbol a linker
// looks for and a FreeRTOS developer greps for, so renaming it to please a
// lint would break the first and hide it from the second.
#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]

//! The FreeRTOS C ABI, over the Kairos kernel.
//!
//! **This file is shared by every C ABI cell**, and is compiled into each
//! of them with `#[path]` rather than linked as a library, because
//! `extern "C"` functions cannot be generic: the kernel's geometry is nine
//! const parameters, and every cell fixes them differently. One copy
//! compiled twice is the honest version of "one seam"; two copies edited
//! in parallel is how a null-handle fix reaches one and not the other.
//!
//! Eight symbols — exactly the set `PollQ.c` needs, and the set was
//! **derived rather than chosen**: the demo file was compiled unmodified
//! against the oracle's own headers and `llvm-nm -u` was asked what it
//! wanted. That list is the specification, and there is no room in it for
//! an opinion.
//!
//! ```text
//! U uxQueueMessagesWaiting   U vTaskDelay            U xQueueGenericSend
//! U vPortEnterCritical       U xQueueGenericCreate   U xQueueReceive
//! U vPortExitCritical        U xTaskCreate
//! ```
//!
//! # The types are the C's, not ours
//!
//! `BaseType_t` is `long` and `UBaseType_t` is `unsigned long` in
//! `portable/GCC/ARM_CM3/portmacro.h`; on this target that is 32 bits.
//! `TickType_t` is `uint32_t` because the config asks for 32-bit ticks.
//! Getting one of these wrong is an ABI bug that a compiler cannot see, so
//! each is written down here beside where it came from.
//!
//! # Handles are indices, not pointers
//!
//! A `TaskHandle_t` is `void *` to C. Ours carries `index + 1`, so a null
//! handle is a failure the way C expects and no kernel address is ever
//! handed across the boundary. That is the same discipline §2.5 applies
//! inside the kernel — handles are indices, never pointers — extended to
//! the seam.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, AtomicU8, AtomicUsize, Ordering};

use rusty_rtos_core::handle::QueueHandle;
use rusty_rtos_kernel_core::queue::{Position, Wait};

// ------------------------------------------- what the CELL must supply --
//
// This file is shared. `firmware/mps2-an385-qemu-capi` compiles it against
// a Cortex-M3 under QEMU; `hosted/capi-host` compiles it against OS
// threads on a laptop. Neither chip nor host appears below this line --
// every platform-shaped thing the seam needs is one of these names, and
// each cell supplies its own:
//
// | name | Cortex-M3 | host |
// |---|---|---|
// | `yield_now` | pend a `PendSV` | hand the run permit over |
// | `without_interrupts` | `cpsid i` / `cpsie i` | take the port's critical lock |
// | `note` | semihosting `hprintln!` | `eprintln!` |
// | `die` | semihosting `debug::exit` | `std::process::exit` |
// | `with_kernel` | the kernel behind a masked-interrupt borrow | the same, behind the same lock |
// | `arm_task` | a static stack plus `init_stack` | an OS thread |
//
// The list being SHORT is the finding. A C ABI over a stackless kernel
// looks as though it must be full of architecture, and it is not: it is
// four verbs and a kernel.
use crate::{arm_task, with_kernel, STACK_WORDS, TASKS};

// ------------------------------------------------------------- C types --

// The C's scalar types and constants live in `rusty_rtos_capi_core::ctypes`,
// beside the header line each came from. They are re-exported rather than
// re-declared: a firmware that spelled `BaseType_t` differently from the
// crate would compile on both sides and disagree at runtime about what a
// word means, which is the one class of ABI bug no compiler can see.
pub use rusty_rtos_capi_core::ctypes::{
    BaseType_t, CopyPosition, StackDepth_t, TickType_t, UBaseType_t, ERR_QUEUE_FULL, PD_FAIL,
    PD_PASS,
};

/// `void (*)( void * )`.
type TaskFunction_t = unsafe extern "C" fn(*mut c_void);

// ------------------------------------------------- the C entry per task --

/// What a task slot has to remember to be able to start a C function: the
/// function, and the `void *` it was given.
///
/// `AtomicUsize`, and the width is the whole point. These were `AtomicU32`
/// while the only cell was a Cortex-M3, where a pointer IS 32 bits, and on
/// a 64-bit host that silently truncated every `pxTaskCode` to its low
/// half: the first task to start jumped into the middle of nowhere and the
/// process died with an access violation before a single line of C ran.
/// Nothing in the type system objected, because on the cell it was written
/// for the two widths were the same.
struct CEntry {
    func: AtomicUsize,
    param: AtomicUsize,
}

const NO_ENTRY: CEntry = CEntry {
    func: AtomicUsize::new(0),
    param: AtomicUsize::new(0),
};
static C_ENTRIES: [CEntry; TASKS] = [NO_ENTRY; TASKS];

/// What each armed task slot holds, for the fault handler: a task that
/// faulted before its entry point was recorded, or one whose saved stack
/// pointer is still zero, is a different bug from one that ran and went
/// wrong, and only the slot table can tell them apart.
pub(crate) fn report_slots() {
    crate::note!(
        "CAPI slots: made={} entered={}",
        TASKS_MADE.load(Ordering::Relaxed),
        TASKS_ENTERED.load(Ordering::Relaxed)
    );
    for i in 0..TASKS {
        // `where_it_runs` is the cell's: a saved stack pointer on the
        // chip, a thread handle on a host. Zero means "armed with
        // nowhere to run", which is the state that caused a fault to
        // `pc = 0` a long way from the call that made it.
        let sp = crate::where_it_runs(i);
        let func = C_ENTRIES[i].func.load(Ordering::Relaxed);
        let param = C_ENTRIES[i].param.load(Ordering::Relaxed);
        if sp == 0 && func == 0 {
            continue;
        }
        crate::note!(
            "    slot {:<3} where={:#010x} func={:#010x} param={:#010x}",
            i,
            sp,
            func,
            param
        );
    }
}

/// One line per call: every task's state and priority, compactly enough
/// that two cells' sequences can be diffed against each other, or one
/// cell's own can be diffed across checks to see WHICH tasks stopped.
///
/// Comparing a failing port against a passing one at the same program
/// point is the only instrument that works here -- the C gives one
/// assertion for a dozen checks, so the question is never "what is wrong"
/// but "where do the two stop agreeing".
pub fn state_line(n: u32) {
    use rusty_rtos_kernel_core::kernel::TaskState;
    let mut out = [0u8; 96];
    let mut len = 0usize;
    for i in 0..TASKS {
        let Some(Some(handle)) = with_kernel(|k| k.task_at(i)) else {
            continue;
        };
        let state = with_kernel(|k| k.task_state_get(handle)).and_then(Result::ok);
        let priority = with_kernel(|k| k.priority_of(Some(handle)))
            .and_then(|r| r.ok())
            .unwrap_or(9);
        let c = match state {
            Some(TaskState::Running) => b'R',
            Some(TaskState::Ready) => b'r',
            Some(TaskState::Blocked) => b'b',
            Some(TaskState::Suspended) => b's',
            Some(TaskState::Deleted) => b'x',
            None => b'?',
        };
        if len + 2 < out.len() {
            out[len] = c;
            out[len + 1] = b'0' + priority.min(9);
            len += 2;
        }
    }
    crate::note!(
        "CAPI state[{:>3}] {}",
        n,
        core::str::from_utf8(&out[..len]).unwrap_or("?")
    );
}

/// What the kernel thought of every task, at the moment something went
/// wrong.
///
/// Most `configASSERT`s in the demo files are about SCHEDULING -- "this
/// task should be blocked by now", "that one should have suspended itself"
/// -- and the assertion alone says only which line disagreed. The state
/// table says what it disagreed with, which is usually the whole
/// diagnosis.
pub(crate) fn report_tasks() {
    use rusty_rtos_kernel_core::kernel::TaskState;
    crate::note!("CAPI task states at the failure:");
    for i in 0..TASKS {
        let func = C_ENTRIES[i].func.load(Ordering::Relaxed);
        if func == 0 && crate::where_it_runs(i) == 0 {
            continue;
        }
        let Some(Some(handle)) = with_kernel(|k| k.task_at(i)) else {
            continue;
        };
        let name = with_kernel(|k| k.name_of(handle)).and_then(Result::ok);
        let state = with_kernel(|k| k.task_state_get(handle)).and_then(Result::ok);
        let priority = with_kernel(|k| k.priority_of(Some(handle))).and_then(Result::ok);
        crate::note!(
            "    task {:<3} {:<18} {:<10} priority {:?}",
            i,
            name.as_ref()
                .map_or("?", rusty_rtos_kernel_core::name::Name::as_str),
            match state {
                Some(TaskState::Running) => "RUNNING",
                Some(TaskState::Ready) => "ready",
                Some(TaskState::Blocked) => "blocked",
                Some(TaskState::Suspended) => "suspended",
                Some(TaskState::Deleted) => "deleted",
                None => "?",
            },
            priority
        );
    }
}

/// Every C task starts here, because `init_stack` hands its entry point a
/// task index and a C task wants its own `void *`.
extern "C" fn c_task_trampoline(index: usize) -> ! {
    let (func, param) = match C_ENTRIES.get(index) {
        Some(e) => (
            e.func.load(Ordering::SeqCst),
            e.param.load(Ordering::SeqCst),
        ),
        None => (0, 0),
    };
    TASKS_ENTERED.fetch_add(1, Ordering::Relaxed);
    if func != 0 {
        // SAFETY: `func` was stored by `xTaskCreate` from a
        // `TaskFunction_t` the C side passed, and is only ever read back
        // here for the slot it was stored against.
        let f: TaskFunction_t = unsafe { core::mem::transmute(func) };
        unsafe { f(param as *mut c_void) };
    }
    // A FreeRTOS task that returns is a bug; the C kernel deletes it. There
    // is nothing to return to, so park rather than fall off the end.
    loop {
        core::hint::spin_loop();
    }
}

/// Per-queue item size, because a Kairos queue carries a `u64` and a
/// FreeRTOS queue copies `uxItemSize` bytes. See [`x_queue_generic_create`].
#[allow(clippy::declare_interior_mutable_const)]
const NO_SIZE: AtomicU32 = AtomicU32::new(0);
static ITEM_SIZES: [AtomicU32; crate::QUEUES] = [NO_SIZE; crate::QUEUES];

/// Which queue handles are queue SETS.
///
/// A set is a queue whose items are the handles of its members, and that
/// makes it the one queue whose contents have to be translated on the way
/// out: the kernel stores `QueueHandle::to_raw()`, while every handle the C
/// holds is `to_raw() + 1`. `xQueueSelectFromSet` did that translation
/// because it returns a handle directly; `xQueuePeek` and `xQueueReceive`
/// did not, because they copy bytes and had no way to know what the bytes
/// meant.
///
/// `QueueSet.c` peeks the set -- `xQueuePeek( xQueueSet, &xReceivedHandle,
/// 0 )` -- and compares the answer against the member handle it was given.
#[allow(clippy::declare_interior_mutable_const)]
const NOT_SET: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
static IS_SET: [core::sync::atomic::AtomicBool; crate::QUEUES] = [NOT_SET; crate::QUEUES];

/// The bytes a set's item occupies on the C side: one `QueueHandle_t`.
const SET_ITEM_BYTES: u32 =
    rusty_rtos_capi_core::codec::set_item_bytes(core::mem::size_of::<*mut c_void>()) as u32;

/// Translate one item read out of `queue` into what the C expects to see.
///
/// For an ordinary queue that is the value itself. For a SET it is a
/// handle, and handles cross this seam as `to_raw() + 1` so that a valid
/// one is never NULL -- while a NULL one stays NULL rather than being
/// manufactured into `1`, which is the same rule `task_to_c` needed.
fn item_for_c(queue: QueueHandle, value: u64) -> u64 {
    match IS_SET.get(usize::from(queue.index())) {
        Some(f) if f.load(Ordering::Relaxed) => rusty_rtos_capi_core::codec::set_item_to_c(value),
        _ => value,
    }
}

/// The widest item this seam can carry, and why.
///
/// A Kairos queue holds a `u64` per slot; a FreeRTOS queue copies an
/// arbitrary `uxItemSize`. Up to eight bytes the two are the same thing and
/// the copy is exact. Beyond that they are not, and
/// [`x_queue_generic_create`] refuses rather than truncating — a queue that
/// silently dropped four bytes of every item would produce a demo that
/// "runs" and is wrong.
use rusty_rtos_capi_core::codec::MAX_ITEM_BYTES;

/// What the C side actually asked for, counted at the seam.
///
/// A demo that reports a failure tells you it is unhappy, not why. These
/// say which call failed and how often, which is the difference between
/// reading the kernel and reading one number.
pub static SENDS_OK: AtomicU32 = AtomicU32::new(0);
pub static SENDS_FAIL: AtomicU32 = AtomicU32::new(0);
pub static RECVS_OK: AtomicU32 = AtomicU32::new(0);
pub static RECVS_FAIL: AtomicU32 = AtomicU32::new(0);
pub static DELAYS: AtomicU32 = AtomicU32::new(0);
pub static TASKS_MADE: AtomicU32 = AtomicU32::new(0);
pub static TASKS_ENTERED: AtomicU32 = AtomicU32::new(0);
pub static QUEUES_MADE: AtomicU32 = AtomicU32::new(0);
pub static EG_MADE: AtomicU32 = AtomicU32::new(0);
pub static EG_GETISR_CALLS: AtomicU32 = AtomicU32::new(0);
pub static EG_GETISR_NONZERO: AtomicU32 = AtomicU32::new(0);
pub static EG_GETISR_ERR: AtomicU32 = AtomicU32::new(0);
pub static EG_SETISR_OK: AtomicU32 = AtomicU32::new(0);
pub static EG_SETISR_FAIL: AtomicU32 = AtomicU32::new(0);
pub static EG_WAIT_OK: AtomicU32 = AtomicU32::new(0);
pub static EG_WAIT_FAIL: AtomicU32 = AtomicU32::new(0);
pub static SEM_GIVE_ISR_OK: AtomicU32 = AtomicU32::new(0);
pub static SEM_GIVE_ISR_FAIL: AtomicU32 = AtomicU32::new(0);
/// ISR-side notifications: attempted, and actually delivered.
pub static NOTIFY_ISR_CALLS: AtomicU32 = AtomicU32::new(0);
pub static NOTIFY_ISR_DELIVERED: AtomicU32 = AtomicU32::new(0);
/// Task-side notification waits: entered, and how many blocked at least once.
pub static NOTIFY_WAITS: AtomicU32 = AtomicU32::new(0);
pub static NOTIFY_WAIT_BLOCKED: AtomicU32 = AtomicU32::new(0);
pub static NOTIFY_TAKES: AtomicU32 = AtomicU32::new(0);
pub static NOTIFY_TAKE_BLOCKED: AtomicU32 = AtomicU32::new(0);
/// The block time of the most recent notification wait, and the tick it
/// started at. A task stuck in one of these is stuck HERE, and the block
/// time says which call site it is.
pub static NOTIFY_LAST_TICKS: AtomicU32 = AtomicU32::new(0);
pub static NOTIFY_LAST_AT: AtomicU32 = AtomicU32::new(0);
/// The software-timer path, step by step: created, started, expired
/// (as the kernel told the hook), and the C callbacks actually run.
pub static TIMERS_MADE: AtomicU32 = AtomicU32::new(0);
pub static TIMERS_STARTED: AtomicU32 = AtomicU32::new(0);
pub static TIMER_EXPIRIES: AtomicU32 = AtomicU32::new(0);
pub static TIMER_CALLBACKS_RUN: AtomicU32 = AtomicU32::new(0);
/// Which arm the daemon's command loop took, and the wait it asked for.
///
/// "Is the daemon parked ON its queue?" has exactly one cheap answer, and
/// it is this: a daemon that is waiting reports `PARKED`, a daemon that is
/// polling reports `POLLED` because the wait it computed was zero. The
/// difference decides whether a task posting a command has anybody to
/// preempt for -- `queue_send_generic` only asks for a switch when the
/// send removes a waiter -- and `TimerDemo` asserts on that switch
/// happening.
pub static DAEMON_WORKED: AtomicU32 = AtomicU32::new(0);
pub static DAEMON_PARKED: AtomicU32 = AtomicU32::new(0);
pub static DAEMON_POLLED: AtomicU32 = AtomicU32::new(0);
pub static DAEMON_FELL_BACK: AtomicU32 = AtomicU32::new(0);
/// Timer commands the queue REFUSED, by command id.
///
/// `TimerDemo` calls `xTimerStop( timer, tmrdemoDONT_BLOCK )` and does not
/// look at the answer -- it asserts on the timer being inactive instead. So
/// a refused command presents as "stop did not work", several lines later
/// and with nothing pointing at the queue.
pub static TIMER_CMD_REFUSED: AtomicU32 = AtomicU32::new(0);
pub static TIMER_CMD_SENT: AtomicU32 = AtomicU32::new(0);
/// `xTaskResumeAll` calls, and how many of them answered `pdTRUE`.
///
/// `dynamic.c` latches an error when a resume it expects to be quiet
/// answers `pdTRUE` ("a yield already happened inside"), so the RATIO is
/// the thing to compare between two ports.
pub static RESUME_ALL: AtomicU32 = AtomicU32::new(0);
pub static RESUME_ALL_YIELDED: AtomicU32 = AtomicU32::new(0);
/// Stream buffers: ISR-side sends, and blocking receives by how many bytes
/// they came back with.
///
/// `StreamBufferDemo`'s trigger-level test asserts that a task blocked on
/// a buffer with trigger level N wakes with EXACTLY N bytes, with no
/// margin -- and no FreeRTOS demo project configures one, so neither do
/// we. A receive that comes back with the wrong count is the whole of that
/// failure, and these say whether it does.
pub static SB_ISR_SENDS: AtomicU32 = AtomicU32::new(0);
pub static SB_BLOCKING_RECVS: AtomicU32 = AtomicU32::new(0);
/// Which buffers are the trigger-level test's.
///
/// `prvInterruptTriggerLevelTest` is the only thing in the demo set that
/// creates a NINE-byte stream buffer, and it is the only test that asserts
/// on the exact byte count a blocked receive comes back with. Tagging it
/// separates its receives from the echo servers' thirty-byte ones, which
/// otherwise bury it.
const TRIGGER_TEST_BYTES: usize = 9;
#[allow(clippy::declare_interior_mutable_const)]
const NOT_TRIGGER: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
static IS_TRIGGER_BUFFER: [core::sync::atomic::AtomicBool; crate::BUFFERS] =
    [NOT_TRIGGER; crate::BUFFERS];
/// Blocking receives on a trigger-test buffer, by the byte count returned.
#[allow(clippy::declare_interior_mutable_const)]
const NO_TRIG: AtomicU32 = AtomicU32::new(0);
pub static SB_TRIGGER_BYTES: [AtomicU32; 10] = [NO_TRIG; 10];
/// The same receives, by how many TICKS they actually blocked.
///
/// One byte arrives per tick, so the byte count and the tick count should
/// agree. Where they do NOT is the whole diagnosis: a receive that blocked
/// its five ticks and came back with six bytes was woken on time and
/// overtaken by the hook, which is the race the demo's own source
/// documents. One that blocked SIX ticks for a five-tick block time is
/// ours.
pub static SB_TRIGGER_TICKS: [AtomicU32; 10] = [NO_TRIG; 10];
/// The tick at which each of the first four OVER-blocking trigger-test
/// receives started. A fixed count that does not grow with run length is a
/// startup transient, and WHEN it happens is the rest of that answer.
pub static SB_OVERBLOCK_AT: [AtomicU32; 4] = [NO_TRIG; 4];
static SB_OVERBLOCKS: AtomicU32 = AtomicU32::new(0);
/// `SB_RECV_BYTES[n]` counts blocking receives that returned `n` bytes,
/// for `n` up to 9 -- the trigger test's buffer is nine bytes.
#[allow(clippy::declare_interior_mutable_const)]
const NO_RECV: AtomicU32 = AtomicU32::new(0);
pub static SB_RECV_BYTES: [AtomicU32; 10] = [NO_RECV; 10];
/// Objects made and given back, per kind.
///
/// The demos create and destroy in loops -- `death.c` tasks, the stream
/// buffer trigger test a buffer per cycle, `TaskNotify.c` a timer per pass
/// -- and every one of them comes out of an arena the twenty-one demos
/// SHARE. A leak in any of these looks from outside exactly like "some
/// demos stop after a while, and which ones varies": whoever asks for the
/// last one loses.
pub static QUEUES_DELETED: AtomicU32 = AtomicU32::new(0);
pub static BUFFERS_MADE: AtomicU32 = AtomicU32::new(0);
pub static BUFFERS_DELETED: AtomicU32 = AtomicU32::new(0);
pub static TIMERS_DELETED: AtomicU32 = AtomicU32::new(0);
pub static TASKS_DELETED: AtomicU32 = AtomicU32::new(0);
static STATE_PROBES: AtomicU32 = AtomicU32::new(0);

// ------------------------------------------------------------ the tasks --

/// `xTaskCreate`.
#[no_mangle]
pub unsafe extern "C" fn xTaskCreate(
    pxTaskCode: TaskFunction_t,
    pcName: *const core::ffi::c_char,
    uxStackDepth: StackDepth_t,
    pvParameters: *mut c_void,
    uxPriority: UBaseType_t,
    pxCreatedTask: *mut *mut c_void,
) -> BaseType_t {
    // A stack this cell cannot give is a refusal, not a smaller stack. The
    // C asked for a depth; silently supplying less is how a task overflows
    // into its neighbour three hours later.
    if uxStackDepth as usize > STACK_WORDS {
        return PD_FAIL;
    }

    let name = c_name(pcName);
    let priority = u8::try_from(uxPriority).unwrap_or(u8::MAX);

    // `create_task` makes the task SCHEDULABLE; `arm_task` gives it a
    // stack. Between those two the task exists, is ready, and has a saved
    // stack pointer of zero -- so a `SysTick` landing in that window can
    // pick it, and the port will restore SP from an empty slot and return
    // through whatever that address holds.
    //
    // It is not a theoretical window. `StreamBufferDemo`'s echo server
    // creates its client at a HIGHER priority than its own, which is the
    // case that makes the new task the obvious next choice, and it faulted
    // to `pc = 0` every run. A task must not be reachable by the scheduler
    // before it has somewhere to run.
    //
    // The whole of it therefore happens with interrupts masked. `with_kernel`
    // masks them per call, which is exactly what left the gap; nesting is
    // safe because `interrupt::free` saves and restores PRIMASK.
    let armed = crate::without_interrupts(|| {
        let Some(Ok(handle)) = with_kernel(|k| k.create_task(name, priority)) else {
            return None;
        };
        let index = usize::from(handle.index());

        let Some(entry) = C_ENTRIES.get(index) else {
            return None;
        };
        entry.func.store(pxTaskCode as usize, Ordering::SeqCst);
        entry.param.store(pvParameters as usize, Ordering::SeqCst);

        if !arm_task(handle, c_task_trampoline) {
            return None;
        }
        Some(handle)
    });
    let Some(handle) = armed else {
        return PD_FAIL;
    };
    if option_env!("KAIROS_CAPI_PROBE") == Some("states") {
        crate::note!(
            "CAPI xTaskCreate {:<18} slot {:<3} asked {} got {:?}",
            name,
            usize::from(handle.index()),
            uxPriority,
            with_kernel(|k| k.priority_of(Some(handle))).and_then(Result::ok)
        );
    }

    if !pxCreatedTask.is_null() {
        // SAFETY: the C side gave us somewhere to write the handle.
        unsafe { *pxCreatedTask = (handle.to_raw() as usize + 1) as *mut c_void };
    }
    TASKS_MADE.fetch_add(1, Ordering::Relaxed);
    PD_PASS
}

/// `vTaskDelay`.
///
/// This is where a stackless kernel and a C task meet. `Kernel::delay`
/// parks the caller and returns; the task must then actually leave the CPU,
/// which `pend_switch` arranges. When the kernel schedules this task again
/// the port restores its stack and execution resumes on the line after —
/// which is exactly what the C expects and what a stackless task cannot do.
#[no_mangle]
pub extern "C" fn vTaskDelay(xTicksToDelay: TickType_t) {
    if xTicksToDelay == 0 {
        return;
    }
    DELAYS.fetch_add(1, Ordering::Relaxed);
    let _ = with_kernel(|k| k.delay(u64::from(xTicksToDelay)));
    crate::yield_now();
}

// ----------------------------------------------------------- the queues --

/// `xQueueGenericCreate`.
#[no_mangle]
pub extern "C" fn xQueueGenericCreate(
    uxQueueLength: UBaseType_t,
    uxItemSize: UBaseType_t,
    _ucQueueType: u8,
) -> *mut c_void {
    if uxItemSize > MAX_ITEM_BYTES {
        return core::ptr::null_mut();
    }
    let Some(Ok(handle)) = with_kernel(|k| k.queue_create(uxQueueLength as usize)) else {
        // The C treats NULL as "no queue" and carries on, usually by
        // skipping whatever it was going to do with it. That is correct
        // FreeRTOS behaviour and a terrible thing to debug, so the seam
        // says out loud what the kernel refused.
        crate::note!(
            "CAPI xQueueCreate REFUSED: length={} item={}B (kernel arena too small?)",
            uxQueueLength,
            uxItemSize
        );
        return core::ptr::null_mut();
    };
    let Some(size) = ITEM_SIZES.get(usize::from(handle.index())) else {
        return core::ptr::null_mut();
    };
    // `ITEM_SIZES` is `u32` because `MAX_ITEM_BYTES` is eight and
    // `xQueueGenericCreate` has already refused anything wider. The cast
    // cannot lose a byte, and `UBaseType_t` is not the same width on every
    // port -- that is the whole reason it is spelled as a cast here rather
    // than assumed to fit.
    size.store(uxItemSize as u32, Ordering::SeqCst);
    QUEUES_MADE.fetch_add(1, Ordering::Relaxed);
    queue_to_c(handle)
}

/// `xQueueGenericSend`.
///
/// `xQueueSend` and `xQueueSendToFront` are both macros over this in
/// `queue.h`, which is why the symbol the demo needs is the generic one.
#[no_mangle]
pub unsafe extern "C" fn xQueueGenericSend(
    xQueue: *mut c_void,
    pvItemToQueue: *const c_void,
    xTicksToWait: TickType_t,
    xCopyPosition: BaseType_t,
) -> BaseType_t {
    let Some(queue) = queue_from_c(xQueue) else {
        return ERR_QUEUE_FULL;
    };
    let Some(size) = ITEM_SIZES.get(usize::from(queue.index())) else {
        return ERR_QUEUE_FULL;
    };
    // SAFETY: the C side promised `uxItemSize` readable bytes, and
    // `xQueueGenericCreate` refused anything wider than eight.
    let value = unsafe { read_item(pvItemToQueue, size.load(Ordering::SeqCst)) };

    // `xQueueSend`, `xQueueSendToFront` and `xQueueOverwrite` are all
    // macros over this one function, separated only by `xCopyPosition`, so
    // an unhandled value here becomes a DIFFERENT OPERATION with no error
    // anywhere. The decode is exhaustive and a value the C never sends is
    // refused rather than rounded to the nearest one we do handle.
    let position = match CopyPosition::decode(xCopyPosition) {
        // Overwrite is its own kernel call, not a position.
        Some(CopyPosition::Overwrite) => {
            return match with_kernel(|k| k.queue_overwrite(queue, value)) {
                Some(Ok(Wait::Ready(()))) => {
                    SENDS_OK.fetch_add(1, Ordering::Relaxed);
                    PD_PASS
                }
                _ => {
                    SENDS_FAIL.fetch_add(1, Ordering::Relaxed);
                    ERR_QUEUE_FULL
                }
            };
        }
        Some(CopyPosition::Front) => Position::Front,
        Some(CopyPosition::Back) => Position::Back,
        None => return ERR_QUEUE_FULL,
    };

    let ticks = u64::from(xTicksToWait);
    let mut spins = 0u32;
    loop {
        match with_kernel(|k| k.queue_send_generic(queue, value, ticks, position)) {
            Some(Ok(Wait::Ready(()))) => {
                SENDS_OK.fetch_add(1, Ordering::Relaxed);
                return PD_PASS;
            }
            // `Blocked` means "call again when this task next runs". With
            // no wait that is simply a full queue; with one, the task has
            // been parked and the switch below is what lets it wait.
            Some(Ok(Wait::Blocked)) => {
                spin_guard(&mut spins, "xQueueGenericSend");
                if ticks == 0 {
                    SENDS_FAIL.fetch_add(1, Ordering::Relaxed);
                    return ERR_QUEUE_FULL;
                }
                crate::yield_now();
            }
            _ => {
                SENDS_FAIL.fetch_add(1, Ordering::Relaxed);
                return ERR_QUEUE_FULL;
            }
        }
    }
}

/// `xQueueReceive`.
#[no_mangle]
pub unsafe extern "C" fn xQueueReceive(
    xQueue: *mut c_void,
    pvBuffer: *mut c_void,
    xTicksToWait: TickType_t,
) -> BaseType_t {
    let Some(queue) = queue_from_c(xQueue) else {
        return PD_FAIL;
    };
    let Some(size) = ITEM_SIZES.get(usize::from(queue.index())) else {
        return PD_FAIL;
    };
    let ticks = u64::from(xTicksToWait);
    let mut spins = 0u32;
    loop {
        match with_kernel(|k| k.queue_receive(queue, ticks)) {
            Some(Ok(Wait::Ready(value))) => {
                let value = item_for_c(queue, value);
                // SAFETY: the C side promised `uxItemSize` writable bytes.
                unsafe { write_item(pvBuffer, value, size.load(Ordering::SeqCst)) };
                RECVS_OK.fetch_add(1, Ordering::Relaxed);
                return PD_PASS;
            }
            Some(Ok(Wait::Blocked)) => {
                spin_guard(&mut spins, "xQueueReceive");
                if ticks == 0 {
                    RECVS_FAIL.fetch_add(1, Ordering::Relaxed);
                    return PD_FAIL;
                }
                crate::yield_now();
            }
            _ => {
                RECVS_FAIL.fetch_add(1, Ordering::Relaxed);
                return PD_FAIL;
            }
        }
    }
}

/// `uxQueueMessagesWaiting`.
#[no_mangle]
pub extern "C" fn uxQueueMessagesWaiting(xQueue: *const c_void) -> UBaseType_t {
    let Some(queue) = queue_from_c(xQueue.cast_mut()) else {
        return 0;
    };
    match with_kernel(|k| k.queue_messages_waiting(queue)) {
        Some(Ok(n)) => n as UBaseType_t,
        _ => 0,
    }
}

// ------------------------------------------------- semaphores and mutexes --

/// `xQueueSemaphoreTake`. `xSemaphoreTake` is a macro over this.
#[no_mangle]
pub extern "C" fn xQueueSemaphoreTake(xQueue: *mut c_void, xTicksToWait: TickType_t) -> BaseType_t {
    let Some(sem) = queue_from_c(xQueue) else {
        return PD_FAIL;
    };
    let ticks = u64::from(xTicksToWait);
    let mut spins = 0u32;
    loop {
        match with_kernel(|k| k.semaphore_take(sem, ticks)) {
            Some(Ok(Wait::Ready(()))) => return PD_PASS,
            Some(Ok(Wait::Blocked)) => {
                spin_guard(&mut spins, "xQueueSemaphoreTake");
                if ticks == 0 {
                    return PD_FAIL;
                }
                crate::yield_now();
            }
            _ => return PD_FAIL,
        }
    }
}

/// `xQueueCreateCountingSemaphore`.
#[no_mangle]
pub extern "C" fn xQueueCreateCountingSemaphore(
    uxMaxCount: UBaseType_t,
    uxInitialCount: UBaseType_t,
) -> *mut c_void {
    match with_kernel(|k| k.semaphore_create_counting(uxMaxCount as usize, uxInitialCount as usize))
    {
        Some(Ok(h)) => queue_to_c(h),
        _ => core::ptr::null_mut(),
    }
}

/// `xQueueCreateMutex`. `xSemaphoreCreateMutex` and its recursive twin are
/// macros over this, distinguished by `ucQueueType`.
#[no_mangle]
pub extern "C" fn xQueueCreateMutex(ucQueueType: u8) -> *mut c_void {
    /// `queueQUEUE_TYPE_RECURSIVE_MUTEX` in `queue.h`.
    const RECURSIVE: u8 = 4;
    let made = if ucQueueType == RECURSIVE {
        with_kernel(|k| k.mutex_create_recursive())
    } else {
        with_kernel(|k| k.mutex_create())
    };
    match made {
        Some(Ok(h)) => queue_to_c(h),
        _ => core::ptr::null_mut(),
    }
}

// -------------------------------------------------------------- the time --

/// `xTaskGetTickCount`.
#[no_mangle]
pub extern "C" fn xTaskGetTickCount() -> TickType_t {
    // The kernel keeps a u64 tick; the C sees the low 32 bits, which is
    // exactly what a 32-bit `TickType_t` would have held all along.
    with_kernel(|k| k.tick_count()).unwrap_or(0) as TickType_t
}

/// `xTaskDelayUntil`.
#[no_mangle]
pub unsafe extern "C" fn xTaskDelayUntil(
    pxPreviousWakeTime: *mut TickType_t,
    xTimeIncrement: TickType_t,
) -> BaseType_t {
    if pxPreviousWakeTime.is_null() {
        return PD_FAIL;
    }
    // SAFETY: the C side owns this and promised it is writable.
    let mut previous = u64::from(unsafe { *pxPreviousWakeTime });
    let delayed = with_kernel(|k| k.delay_until(&mut previous, u64::from(xTimeIncrement)));
    // SAFETY: as above.
    unsafe { *pxPreviousWakeTime = previous as TickType_t };
    match delayed {
        Some(Ok(true)) => {
            crate::yield_now();
            PD_PASS
        }
        // The wake time had already passed: the C expects pdFALSE and no
        // block, which is what not switching here means.
        Some(Ok(false)) => PD_FAIL,
        _ => PD_FAIL,
    }
}

// ------------------------------------------------------- suspend / resume --

/// `vTaskSuspend`. A NULL handle means the caller, as in the C.
#[no_mangle]
pub extern "C" fn vTaskSuspend(xTaskToSuspend: *mut c_void) {
    let who = task_from_c(xTaskToSuspend);
    let _ = with_kernel(|k| k.suspend(who));
    // Suspending yourself has to take effect before the call returns, and
    // the only way for that to be true is to leave the CPU here.
    if who.is_none() {
        crate::yield_now();
    }
}

/// `vTaskResume`.
#[no_mangle]
pub extern "C" fn vTaskResume(xTaskToResume: *mut c_void) {
    if let Some(task) = task_from_c(xTaskToResume) {
        let _ = with_kernel(|k| k.resume(task));
        crate::yield_now();
    }
}

/// `xQueuePeek`.
#[no_mangle]
pub unsafe extern "C" fn xQueuePeek(
    xQueue: *mut c_void,
    pvBuffer: *mut c_void,
    xTicksToWait: TickType_t,
) -> BaseType_t {
    let Some(queue) = queue_from_c(xQueue) else {
        return PD_FAIL;
    };
    let Some(size) = ITEM_SIZES.get(usize::from(queue.index())) else {
        return PD_FAIL;
    };
    let ticks = u64::from(xTicksToWait);
    let mut spins = 0u32;
    loop {
        match with_kernel(|k| k.queue_peek(queue, ticks)) {
            Some(Ok(Wait::Ready(value))) => {
                let value = item_for_c(queue, value);
                // SAFETY: the C promised `uxItemSize` writable bytes.
                unsafe { write_item(pvBuffer, value, size.load(Ordering::SeqCst)) };
                return PD_PASS;
            }
            Some(Ok(Wait::Blocked)) => {
                spin_guard(&mut spins, "xQueuePeek");
                if ticks == 0 {
                    return PD_FAIL;
                }
                crate::yield_now();
            }
            _ => return PD_FAIL,
        }
    }
}

// ---------------------------------------------------------------- heap --

/// `portBYTE_ALIGNMENT` for ARM_CM3, and `sizeof( BlockLink_t )` on a
/// 32-bit target: one pointer plus one `size_t`.
type DemoHeap = rusty_rtos_heap_core::heap4::Heap4<8192, 8, 8>;

struct HeapCell(core::cell::UnsafeCell<DemoHeap>);
// SAFETY: every access goes through `with_heap`, which masks interrupts.
unsafe impl Sync for HeapCell {}
static HEAP: HeapCell = HeapCell(core::cell::UnsafeCell::new(DemoHeap::new()));

fn with_heap<R>(f: impl FnOnce(&mut DemoHeap) -> R) -> R {
    crate::without_interrupts(|| {
        // SAFETY: interrupts are masked and there is one core, so no other
        // holder exists for the duration of `f`.
        f(unsafe { &mut *HEAP.0.get() })
    })
}

/// `pvPortMalloc`.
///
/// This is **K4's `heap_4` remake**, not a new allocator written for the
/// ABI. That matters: the allocator the C demos are about to hammer is the
/// one already diffed against `heap_4.c` over 20,000 operations, agreeing
/// on the offset first fit chose, the free bytes remaining and the minimum
/// ever free. Writing a second one here would have thrown that away.
#[no_mangle]
pub extern "C" fn pvPortMalloc(xWantedSize: usize) -> *mut c_void {
    with_heap(|h| match h.alloc(xWantedSize).map(|block| block.offset()) {
        Some(offset) => match h.address_of(offset) {
            Some(p) => p.as_ptr().cast::<c_void>(),
            None => core::ptr::null_mut(),
        },
        None => core::ptr::null_mut(),
    })
}

/// `vPortFree`.
#[no_mangle]
pub extern "C" fn vPortFree(pv: *mut c_void) {
    if pv.is_null() {
        return;
    }
    with_heap(|h| {
        // A pointer from somewhere else is refused rather than corrupting
        // the free list; `offset_of` bounds-checks against the arena.
        //
        // `free_raw`, NOT `free`, and the difference is the C's type rather
        // than a shortcut. `Heap4::free` takes a `Block` — offset plus the
        // generation it was handed out under — so it can refuse a free of a
        // block that has since been REALLOCATED. `vPortFree( void * )` has
        // nowhere to put a generation; an address is all the C has, which is
        // exactly why `heap_4.c` manages only the allocated-bit check itself.
        //
        // So a C caller gets what FreeRTOS gives it: a double free is caught,
        // a stale free is not. A Rust caller gets the stronger guarantee. The
        // limit is the C's type system, not this heap, and blurring it would
        // claim an ABI guarantee that cannot be delivered.
        if let Some(offset) = h.offset_of(pv.cast::<u8>()) {
            let _ = h.free_raw(offset);
        }
    });
}

/// `xPortGetFreeHeapSize`, so a demo can ask.
#[no_mangle]
pub extern "C" fn xPortGetFreeHeapSize() -> usize {
    with_heap(|h| h.free_bytes())
}

// --------------------------------------------------------- the critical --

/// `vPortEnterCritical`. The demo enters one around its shared counters.
#[no_mangle]
pub extern "C" fn vPortEnterCritical() {
    let _ = with_kernel(|k| k.enter_critical());
}

/// `vPortExitCritical`.
#[no_mangle]
pub extern "C" fn vPortExitCritical() {
    let _ = with_kernel(|k| k.exit_critical());
}

/// `vPortGenerateSimulatedInterrupt`: `taskYIELD()` on a host port.
///
/// The surface is derived from what the demo files ask for, and what they
/// ask for depends on the PORT HEADER they are compiled against -- which is
/// a thing the derivation has to be allowed to notice. `ARM_CM3`'s
/// `portmacro.h` expands `portYIELD()` into a write to the SCB, so the
/// Cortex-M cell needed no symbol for it. `MSVC-MingW`'s expands it into
/// `vPortGenerateSimulatedInterrupt( portINTERRUPT_YIELD )`, so the host
/// cell does: `semtest.c` calls `taskYIELD()` and would not link without
/// it.
///
/// The interrupt number is ignored because this port has exactly one
/// reason to switch and no interrupt controller to name. It is exported
/// from the shared seam rather than from the host cell so that the symbol
/// table stays ONE list -- a symbol that exists in one cell and not the
/// other is how a generated header stops describing the ABI.
#[no_mangle]
pub extern "C" fn vPortGenerateSimulatedInterrupt(_ulInterruptNumber: u32) {
    crate::yield_now();
}

/// How many times a retry loop may go round without progress before it is
/// called a hang.
///
/// A genuinely blocking call costs one iteration per *wakeup*, because the
/// kernel does not schedule a task whose `Wait` is still `Blocked` -- so a
/// real block reaches single digits. Reaching a million means the loop is
/// spinning, not waiting.
use rusty_rtos_capi_core::retry::SPIN_LIMIT;

/// A retry loop that makes no progress is this seam's characteristic
/// failure, and from outside it is indistinguishable from a deadlock, a
/// starved monitor, or a fault loop: the run simply stops. Worse, if the
/// loop was entered inside a critical section then the tick that would
/// unblock it can never fire, so the hang is self-sealing.
///
/// The guard costs one increment per blocked iteration and converts all of
/// that into one line naming the function that stopped.
#[inline]
fn spin_guard(spins: &mut u32, what: &str) {
    *spins += 1;
    if *spins == SPIN_LIMIT {
        crate::note!(
            "CAPI SPIN -- {} retried {} times without progress: the retry loop is              spinning, not blocking.",
            what,
            SPIN_LIMIT
        );
        crate::die();
    }
}

/// `configASSERT` failing is the most valuable thing this cell can report:
/// it is the C telling us the ABI lied to it.
#[no_mangle]
pub unsafe extern "C" fn vCapiAssertFailed(file: *const core::ffi::c_char, line: u32) {
    // A longer bound than `c_name`'s: this is a path, and truncating it at
    // 32 characters hides which file the C is complaining about, which is
    // the only thing the message is for.
    let mut len = 0usize;
    // SAFETY: bounded by 200; the string is NUL-terminated or truncated.
    while len < 200 && unsafe { *file.add(len) } != 0 {
        len += 1;
    }
    // SAFETY: `len` bytes are readable, by the loop above.
    let bytes = unsafe { core::slice::from_raw_parts(file.cast::<u8>(), len) };
    let path = core::str::from_utf8(bytes).unwrap_or("<not utf8>");
    crate::note!("CAPI configASSERT FAILED at {}:{}", path, line);
    // The timer daemon's posture, because several of the C's assertions
    // depend on it PREEMPTING the asserting task and a polling daemon
    // cannot. `polled` above zero is the answer to "why did my
    // higher-priority daemon not run".
    crate::note!(
        "CAPI daemon: worked={} parked={} polled={} fell back={}",
        DAEMON_WORKED.load(Ordering::Relaxed),
        DAEMON_PARKED.load(Ordering::Relaxed),
        DAEMON_POLLED.load(Ordering::Relaxed),
        DAEMON_FELL_BACK.load(Ordering::Relaxed)
    );
    crate::note!(
        "CAPI timer commands: sent={} refused={}",
        TIMER_CMD_SENT.load(Ordering::Relaxed),
        TIMER_CMD_REFUSED.load(Ordering::Relaxed)
    );
    report_tasks();
    crate::die();
}

// ----------------------------------------------------------- the plumbing --

/// A `void *` handle back to the kernel handle it carries.
///
/// The C side holds `to_raw() + 1`, not an index. The `+ 1` is what makes
/// a null pointer mean "no handle" the way C expects; the RAW value is what
/// carries the generation counter, so a handle to a deleted object is
/// rejected by the kernel rather than silently resolving to whatever now
/// occupies that slot. Passing a bare index would have thrown that away.
///
/// There were FIVE hand-written copies of this encode in this file, one per
/// handle kind, and only one of them had the NULL guard -- the one
/// `GenQTest.c:564` had already found. The other four carried the same
/// defect, latent, waiting for a demo that asked a queue, a timer, a group
/// or a buffer for a handle it did not have. A rule written five times is
/// a rule fixed once, so these delegate to `rusty_rtos_capi_core::codec`,
/// which has the tests.
pub(crate) fn handle_from_c<K: rusty_rtos_core::handle::Kind>(
    handle: *mut c_void,
) -> Option<rusty_rtos_core::handle::Handle<K>> {
    rusty_rtos_capi_core::codec::handle_from_c(handle as usize)
}

/// The C `void *` for a kernel handle of any kind.
pub(crate) fn handle_to_c<K: rusty_rtos_core::handle::Kind>(
    handle: rusty_rtos_core::handle::Handle<K>,
) -> *mut c_void {
    rusty_rtos_capi_core::codec::handle_to_c(handle) as *mut c_void
}

fn queue_from_c(handle: *mut c_void) -> Option<QueueHandle> {
    handle_from_c(handle)
}

/// A `void *` task handle back to the kernel's, or `None` for NULL — which
/// means "the calling task" throughout the FreeRTOS API.
fn task_from_c(handle: *mut c_void) -> Option<rusty_rtos_core::handle::TaskHandle> {
    handle_from_c(handle)
}

fn queue_to_c(handle: QueueHandle) -> *mut c_void {
    handle_to_c(handle)
}

/// Read up to eight bytes of a C item into the `u64` a Kairos queue holds.
///
/// # Safety
/// `src` must point at `bytes` readable bytes, and `bytes` must be <= 8.
unsafe fn read_item(src: *const c_void, bytes: u32) -> u64 {
    let n = (bytes as usize).min(8);
    if src.is_null() || n == 0 {
        return 0;
    }
    // SAFETY: the caller promised `bytes` readable bytes, and `n` is that
    // capped at the slot width. The DECISION -- little-endian, zero
    // extended, never wider than the slot -- belongs to `codec` and is
    // tested there; this line is only the pointer.
    let bytes = unsafe { core::slice::from_raw_parts(src.cast::<u8>(), n) };
    rusty_rtos_capi_core::codec::item_from_bytes(bytes)
}

/// The inverse.
///
/// # Safety
/// `dst` must point at `bytes` writable bytes, and `bytes` must be <= 8.
unsafe fn write_item(dst: *mut c_void, value: u64, bytes: u32) {
    let n = (bytes as usize).min(8);
    if dst.is_null() || n == 0 {
        return;
    }
    // SAFETY: as `read_item`, in the other direction.
    let out = unsafe { core::slice::from_raw_parts_mut(dst.cast::<u8>(), n) };
    rusty_rtos_capi_core::codec::item_to_bytes(value, out);
}

/// A NUL-terminated C name as a `&str`, bounded so a missing terminator
/// cannot walk off the end.
fn c_name<'a>(name: *const core::ffi::c_char) -> &'a str {
    // SAFETY: the scan is bounded by `MAX_NAME_BYTES`, so an unterminated
    // string is truncated rather than read past. What "truncated" MEANS --
    // where the NUL search stops, what happens to bytes that are not
    // UTF-8 -- belongs to `cstr` and is tested there.
    unsafe { c_str(name, rusty_rtos_capi_core::cstr::MAX_NAME_BYTES) }
}

/// A NUL-terminated C string as a `&str`, scanning at most `limit` bytes.
///
/// # Safety
/// `name` is NULL, or points at readable bytes up to its NUL or `limit`.
unsafe fn c_str<'a>(name: *const core::ffi::c_char, limit: usize) -> &'a str {
    if name.is_null() {
        return "";
    }
    let mut len = 0usize;
    // SAFETY: bounded by `limit`; the caller's string is either shorter
    // and NUL-terminated, or truncated here rather than read past.
    while len < limit && unsafe { *name.add(len) } != 0 {
        len += 1;
    }
    // SAFETY: `len` bytes are readable, by the loop above.
    let bytes = unsafe { core::slice::from_raw_parts(name.cast::<u8>(), len) };
    rusty_rtos_capi_core::cstr::name_from(bytes, limit)
}

// ============================================================ the tasks ==

/// `vTaskDelete`. A NULL handle means the caller, as in the C.
#[no_mangle]
pub extern "C" fn vTaskDelete(xTaskToDelete: *mut c_void) {
    TASKS_DELETED.fetch_add(1, Ordering::Relaxed);
    let who = task_from_c(xTaskToDelete);
    let _ = with_kernel(|k| k.task_delete(who));
    if who.is_none() {
        // Deleting yourself must not return to the caller's next line.
        crate::yield_now();
    }
}

/// `vTaskPrioritySet`.
#[no_mangle]
pub extern "C" fn vTaskPrioritySet(xTask: *mut c_void, uxNewPriority: UBaseType_t) {
    let who = task_from_c(xTask);
    let priority = u8::try_from(uxNewPriority).unwrap_or(u8::MAX);
    if option_env!("KAIROS_CAPI_PROBE") == Some("states") {
        let (caller, target) = with_kernel(|k| {
            (
                k.name_of(k.current()).ok(),
                k.name_of(who.unwrap_or_else(|| k.current())).ok(),
            )
        })
        .unwrap_or((None, None));
        crate::note!(
            "CAPI vTaskPrioritySet caller={:?} target={:?} -> {}",
            caller
                .as_ref()
                .map(rusty_rtos_kernel_core::name::Name::as_str),
            target
                .as_ref()
                .map(rusty_rtos_kernel_core::name::Name::as_str),
            priority
        );
    }
    let _ = with_kernel(|k| k.set_priority(who, priority));
    // Raising another task above us, or lowering ourselves, both mean the
    // running task may no longer be the one that should run.
    crate::yield_now();
}

/// `uxTaskPriorityGet`.
#[no_mangle]
pub extern "C" fn uxTaskPriorityGet(xTask: *const c_void) -> UBaseType_t {
    match with_kernel(|k| k.task_priority_get(task_from_c(xTask.cast_mut()))) {
        Some(Ok(p)) => UBaseType_t::from(p),
        _ => 0,
    }
}

/// `eTaskGetState`.
///
/// The `eTaskState` values are `queue.h`'s order and must match exactly:
/// eRunning, eReady, eBlocked, eSuspended, eDeleted, eInvalid.
#[no_mangle]
pub extern "C" fn eTaskGetState(xTask: *mut c_void) -> u32 {
    use rusty_rtos_kernel_core::kernel::TaskState;
    // A one-shot dump at the first `eTaskGetState`, so two cells can be
    // compared at the SAME program point. Several demos assert on this
    // call and on nothing else nearby, which makes it a good place to
    // stand: `KAIROS_CAPI_PROBE=states`.
    if option_env!("KAIROS_CAPI_PROBE") == Some("states") {
        let n = STATE_PROBES.fetch_add(1, Ordering::Relaxed);
        if n < 60 {
            state_line(n);
        }
    }
    let Some(task) = task_from_c(xTask) else {
        return 5; // eInvalid
    };
    match with_kernel(|k| k.task_state_get(task)) {
        Some(Ok(TaskState::Running)) => 0,
        Some(Ok(TaskState::Ready)) => 1,
        Some(Ok(TaskState::Blocked)) => 2,
        Some(Ok(TaskState::Suspended)) => 3,
        Some(Ok(TaskState::Deleted)) => 4,
        _ => 5,
    }
}

/// `xTaskGetHandle`.
#[no_mangle]
pub unsafe extern "C" fn xTaskGetHandle(pcNameToQuery: *const core::ffi::c_char) -> *mut c_void {
    let name = c_name(pcNameToQuery);
    match with_kernel(|k| k.task_get_handle(name)) {
        Some(Ok(h)) => task_to_c(h),
        _ => core::ptr::null_mut(),
    }
}

/// `xTaskGetCurrentTaskHandle`.
#[no_mangle]
pub extern "C" fn xTaskGetCurrentTaskHandle() -> *mut c_void {
    match with_kernel(|k| k.current()) {
        Some(h) => task_to_c(h),
        None => core::ptr::null_mut(),
    }
}

/// `xTaskAbortDelay`.
#[no_mangle]
pub extern "C" fn xTaskAbortDelay(xTask: *mut c_void) -> BaseType_t {
    let Some(task) = task_from_c(xTask) else {
        return PD_FAIL;
    };
    match with_kernel(|k| k.abort_delay(task)) {
        Some(Ok(true)) => {
            crate::yield_now();
            PD_PASS
        }
        _ => PD_FAIL,
    }
}

/// `uxTaskGetNumberOfTasks`.
#[no_mangle]
pub extern "C" fn uxTaskGetNumberOfTasks() -> UBaseType_t {
    with_kernel(|k| k.task_count()).unwrap_or(0) as UBaseType_t
}

/// `vTaskSuspendAll`.
#[no_mangle]
pub extern "C" fn vTaskSuspendAll() {
    let _ = with_kernel(|k| k.suspend_all());
}

/// `xTaskResumeAll`.
#[no_mangle]
pub extern "C" fn xTaskResumeAll() -> BaseType_t {
    RESUME_ALL.fetch_add(1, Ordering::Relaxed);
    match with_kernel(|k| k.resume_all()) {
        Some(true) => {
            RESUME_ALL_YIELDED.fetch_add(1, Ordering::Relaxed);
            crate::yield_now();
            PD_PASS
        }
        _ => PD_FAIL,
    }
}

/// `xTaskCatchUpTicks`: used by the timer tests to jump time forward.
#[no_mangle]
pub extern "C" fn xTaskCatchUpTicks(xTicksToCatchUp: TickType_t) -> BaseType_t {
    let mut woken = false;
    for _ in 0..xTicksToCatchUp {
        woken |= with_kernel(|k| k.increment_tick()).unwrap_or(false);
    }
    if woken {
        crate::yield_now();
        PD_PASS
    } else {
        PD_FAIL
    }
}

/// `xTaskGetTickCountFromISR`.
#[no_mangle]
pub extern "C" fn xTaskGetTickCountFromISR() -> TickType_t {
    with_kernel(|k| k.tick_count()).unwrap_or(0) as TickType_t
}

// ==================================================== the notifications ==

/// `eNotifyAction`, in `task.h`'s order.
fn notify_action(action: u32) -> Option<rusty_rtos_kernel_core::kernel::NotifyAction> {
    use rusty_rtos_kernel_core::kernel::NotifyAction as A;
    Some(match action {
        0 => A::None,
        1 => A::SetBits,
        2 => A::Increment,
        3 => A::Overwrite,
        4 => A::NoOverwrite,
        _ => return None,
    })
}

/// `xTaskGenericNotify`.
#[no_mangle]
pub unsafe extern "C" fn xTaskGenericNotify(
    xTaskToNotify: *mut c_void,
    uxIndexToNotify: UBaseType_t,
    ulValue: u32,
    eAction: u32,
    pulPreviousNotificationValue: *mut u32,
) -> BaseType_t {
    let (Some(task), Some(action)) = (task_from_c(xTaskToNotify), notify_action(eAction)) else {
        return PD_FAIL;
    };
    // `xTaskNotifyAndQuery` is this function with the out-parameter, and
    // it must answer the value the notification is about to REPLACE --
    // `TaskNotify.c:373` asserts on it. Read it BEFORE notifying.
    if !pulPreviousNotificationValue.is_null() {
        let previous = with_kernel(|k| k.notify_value(Some(task), uxIndexToNotify as usize));
        // SAFETY: the C gave us somewhere to write it.
        unsafe {
            *pulPreviousNotificationValue = match previous {
                Some(Ok(v)) => v,
                _ => 0,
            }
        };
    }
    // The kernel's `bool` is the ANSWER, not a formality: with
    // `eSetValueWithoutOverwrite` a notification that is already pending
    // makes this fail, and `TaskNotify.c:204` asserts exactly that.
    // Discarding it returned pdPASS for a notification that never landed.
    match with_kernel(|k| k.notify(task, uxIndexToNotify as usize, ulValue, action)) {
        Some(Ok(true)) => {
            crate::yield_now();
            PD_PASS
        }
        _ => PD_FAIL,
    }
}

/// `xTaskGenericNotifyWait`.
#[no_mangle]
pub unsafe extern "C" fn xTaskGenericNotifyWait(
    uxIndexToWaitOn: UBaseType_t,
    ulBitsToClearOnEntry: u32,
    ulBitsToClearOnExit: u32,
    pulNotificationValue: *mut u32,
    xTicksToWait: TickType_t,
) -> BaseType_t {
    let index = uxIndexToWaitOn as usize;
    let ticks = u64::from(xTicksToWait);
    let mut spins = 0u32;
    NOTIFY_WAITS.fetch_add(1, Ordering::Relaxed);
    NOTIFY_LAST_TICKS.store(xTicksToWait, Ordering::Relaxed);
    NOTIFY_LAST_AT.store(
        with_kernel(|k| k.tick_count()).unwrap_or(0) as u32,
        Ordering::Relaxed,
    );
    loop {
        match with_kernel(|k| {
            k.notify_wait(index, ulBitsToClearOnEntry, ulBitsToClearOnExit, ticks)
        }) {
            Some(Ok(Wait::Ready((got, value)))) => {
                if !pulNotificationValue.is_null() {
                    // SAFETY: the C gave us somewhere to write it.
                    unsafe { *pulNotificationValue = value };
                }
                return if got { PD_PASS } else { PD_FAIL };
            }
            Some(Ok(Wait::Blocked)) => {
                NOTIFY_WAIT_BLOCKED.fetch_add(1, Ordering::Relaxed);
                spin_guard(&mut spins, "xTaskGenericNotifyWait");
                if ticks == 0 {
                    return PD_FAIL;
                }
                crate::yield_now();
            }
            _ => return PD_FAIL,
        }
    }
}

/// `ulTaskGenericNotifyTake`.
#[no_mangle]
pub extern "C" fn ulTaskGenericNotifyTake(
    uxIndexToWaitOn: UBaseType_t,
    xClearCountOnExit: BaseType_t,
    xTicksToWait: TickType_t,
) -> u32 {
    let index = uxIndexToWaitOn as usize;
    let ticks = u64::from(xTicksToWait);
    let mut spins = 0u32;
    NOTIFY_TAKES.fetch_add(1, Ordering::Relaxed);
    loop {
        match with_kernel(|k| k.notify_take(index, xClearCountOnExit != 0, ticks)) {
            Some(Ok(Wait::Ready(value))) => return value,
            Some(Ok(Wait::Blocked)) => {
                NOTIFY_TAKE_BLOCKED.fetch_add(1, Ordering::Relaxed);
                spin_guard(&mut spins, "ulTaskGenericNotifyTake");
                if ticks == 0 {
                    return 0;
                }
                crate::yield_now();
            }
            _ => return 0,
        }
    }
}

/// `xTaskGenericNotifyStateClear`.
#[no_mangle]
pub extern "C" fn xTaskGenericNotifyStateClear(
    xTask: *mut c_void,
    uxIndexToClear: UBaseType_t,
) -> BaseType_t {
    match with_kernel(|k| k.notify_state_clear(task_from_c(xTask), uxIndexToClear as usize)) {
        Some(Ok(true)) => PD_PASS,
        _ => PD_FAIL,
    }
}

/// `ulTaskGenericNotifyValueClear`.
#[no_mangle]
pub extern "C" fn ulTaskGenericNotifyValueClear(
    xTask: *mut c_void,
    uxIndexToClear: UBaseType_t,
    ulBitsToClear: u32,
) -> u32 {
    match with_kernel(|k| {
        k.notify_value_clear(task_from_c(xTask), uxIndexToClear as usize, ulBitsToClear)
    }) {
        Some(Ok(before)) => before,
        _ => 0,
    }
}

/// `xTaskGenericNotifyFromISR`.
#[no_mangle]
pub unsafe extern "C" fn xTaskGenericNotifyFromISR(
    xTaskToNotify: *mut c_void,
    uxIndexToNotify: UBaseType_t,
    ulValue: u32,
    eAction: u32,
    pulPreviousNotificationValue: *mut u32,
    pxHigherPriorityTaskWoken: *mut BaseType_t,
) -> BaseType_t {
    let (Some(task), Some(action)) = (task_from_c(xTaskToNotify), notify_action(eAction)) else {
        return PD_FAIL;
    };
    if !pulPreviousNotificationValue.is_null() {
        let previous = with_kernel(|k| k.notify_value(Some(task), uxIndexToNotify as usize));
        // SAFETY: the C gave us somewhere to write it.
        unsafe {
            *pulPreviousNotificationValue = match previous {
                Some(Ok(v)) => v,
                _ => 0,
            }
        };
    }
    NOTIFY_ISR_CALLS.fetch_add(1, Ordering::Relaxed);
    match with_kernel(|k| k.notify_from_isr(task, uxIndexToNotify as usize, ulValue, action)) {
        Some(Ok((delivered, woken))) => {
            if delivered {
                NOTIFY_ISR_DELIVERED.fetch_add(1, Ordering::Relaxed);
            }
            // SAFETY: the caller's own flag, if it passed one.
            unsafe { set_woken(pxHigherPriorityTaskWoken, woken) };
            if delivered {
                PD_PASS
            } else {
                PD_FAIL
            }
        }
        _ => PD_FAIL,
    }
}

/// `vTaskGenericNotifyGiveFromISR`.
#[no_mangle]
pub unsafe extern "C" fn vTaskGenericNotifyGiveFromISR(
    xTaskToNotify: *mut c_void,
    uxIndexToNotify: UBaseType_t,
    pxHigherPriorityTaskWoken: *mut BaseType_t,
) {
    use rusty_rtos_kernel_core::kernel::NotifyAction;
    let Some(task) = task_from_c(xTaskToNotify) else {
        return;
    };
    NOTIFY_ISR_CALLS.fetch_add(1, Ordering::Relaxed);
    if let Some(Ok((delivered, woken))) = with_kernel(|k| {
        k.notify_from_isr(task, uxIndexToNotify as usize, 0, NotifyAction::Increment)
    }) {
        if delivered {
            NOTIFY_ISR_DELIVERED.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: the caller's own flag, if it passed one.
        unsafe { set_woken(pxHigherPriorityTaskWoken, woken) };
    }
}

// ========================================================== more queues ==

/// `vQueueDelete`.
#[no_mangle]
pub extern "C" fn vQueueDelete(xQueue: *mut c_void) {
    QUEUES_DELETED.fetch_add(1, Ordering::Relaxed);
    if let Some(q) = queue_from_c(xQueue) {
        let _ = with_kernel(|k| k.queue_delete(q));
    }
}

/// `uxQueueSpacesAvailable`.
#[no_mangle]
pub extern "C" fn uxQueueSpacesAvailable(xQueue: *const c_void) -> UBaseType_t {
    let Some(q) = queue_from_c(xQueue.cast_mut()) else {
        return 0;
    };
    match with_kernel(|k| k.queue_spaces_available(q)) {
        Some(Ok(n)) => n as UBaseType_t,
        _ => 0,
    }
}

/// `xQueueGenericReset`.
#[no_mangle]
pub extern "C" fn xQueueGenericReset(xQueue: *mut c_void, _xNewQueue: BaseType_t) -> BaseType_t {
    let Some(q) = queue_from_c(xQueue) else {
        return PD_FAIL;
    };
    match with_kernel(|k| k.queue_reset(q)) {
        Some(Ok(())) => PD_PASS,
        _ => PD_FAIL,
    }
}

/// `xQueueGenericSendFromISR`.
#[no_mangle]
pub unsafe extern "C" fn xQueueGenericSendFromISR(
    xQueue: *mut c_void,
    pvItemToQueue: *const c_void,
    pxHigherPriorityTaskWoken: *mut BaseType_t,
    xCopyPosition: BaseType_t,
) -> BaseType_t {
    let Some(q) = queue_from_c(xQueue) else {
        return ERR_QUEUE_FULL;
    };
    let Some(size) = ITEM_SIZES.get(usize::from(q.index())) else {
        return ERR_QUEUE_FULL;
    };
    // SAFETY: the C promised `uxItemSize` readable bytes.
    let value = unsafe { read_item(pvItemToQueue, size.load(Ordering::SeqCst)) };
    // As the task-side form: every position decoded, none inferred.
    let position = match CopyPosition::decode(xCopyPosition) {
        Some(CopyPosition::Overwrite) => {
            return match with_kernel(|k| k.queue_overwrite_from_isr(q, value)) {
                Some(Ok(woken)) => {
                    // SAFETY: the caller's own flag, if it passed one.
                    unsafe { set_woken(pxHigherPriorityTaskWoken, woken) };
                    PD_PASS
                }
                _ => ERR_QUEUE_FULL,
            };
        }
        Some(CopyPosition::Front) => Position::Front,
        Some(CopyPosition::Back) => Position::Back,
        None => return ERR_QUEUE_FULL,
    };
    match with_kernel(|k| k.queue_send_generic_from_isr(q, value, position)) {
        Some(Ok(woken)) => {
            // SAFETY: the caller's own flag, if it passed one.
            unsafe { set_woken(pxHigherPriorityTaskWoken, woken) };
            PD_PASS
        }
        _ => ERR_QUEUE_FULL,
    }
}

/// `xQueueReceiveFromISR`.
#[no_mangle]
pub unsafe extern "C" fn xQueueReceiveFromISR(
    xQueue: *mut c_void,
    pvBuffer: *mut c_void,
    pxHigherPriorityTaskWoken: *mut BaseType_t,
) -> BaseType_t {
    let Some(q) = queue_from_c(xQueue) else {
        return PD_FAIL;
    };
    let Some(size) = ITEM_SIZES.get(usize::from(q.index())) else {
        return PD_FAIL;
    };
    match with_kernel(|k| k.queue_receive_from_isr(q)) {
        Some(Ok((value, woken))) => {
            let value = item_for_c(q, value);
            // SAFETY: the C promised `uxItemSize` writable bytes.
            unsafe { write_item(pvBuffer, value, size.load(Ordering::SeqCst)) };
            // SAFETY: the caller's own flag, if it passed one.
            unsafe { set_woken(pxHigherPriorityTaskWoken, woken) };
            PD_PASS
        }
        _ => PD_FAIL,
    }
}

/// `xQueuePeekFromISR`.
#[no_mangle]
pub unsafe extern "C" fn xQueuePeekFromISR(
    xQueue: *mut c_void,
    pvBuffer: *mut c_void,
) -> BaseType_t {
    let Some(q) = queue_from_c(xQueue) else {
        return PD_FAIL;
    };
    let Some(size) = ITEM_SIZES.get(usize::from(q.index())) else {
        return PD_FAIL;
    };
    match with_kernel(|k| k.queue_peek_from_isr(q)) {
        Some(Ok(value)) => {
            let value = item_for_c(q, value);
            // SAFETY: the C promised `uxItemSize` writable bytes.
            unsafe { write_item(pvBuffer, value, size.load(Ordering::SeqCst)) };
            PD_PASS
        }
        _ => PD_FAIL,
    }
}

/// `xQueueGiveFromISR`.
#[no_mangle]
pub unsafe extern "C" fn xQueueGiveFromISR(
    xQueue: *mut c_void,
    pxHigherPriorityTaskWoken: *mut BaseType_t,
) -> BaseType_t {
    let Some(q) = queue_from_c(xQueue) else {
        return PD_FAIL;
    };
    match with_kernel(|k| k.semaphore_give_from_isr(q)) {
        Some(Ok(woken)) => {
            // SAFETY: the caller's own flag, if it passed one.
            unsafe { set_woken(pxHigherPriorityTaskWoken, woken) };
            SEM_GIVE_ISR_OK.fetch_add(1, Ordering::Relaxed);
            PD_PASS
        }
        _ => {
            SEM_GIVE_ISR_FAIL.fetch_add(1, Ordering::Relaxed);
            PD_FAIL
        }
    }
}

/// `xQueueGetMutexHolder`.
#[no_mangle]
pub extern "C" fn xQueueGetMutexHolder(xSemaphore: *mut c_void) -> *mut c_void {
    let Some(m) = queue_from_c(xSemaphore) else {
        return core::ptr::null_mut();
    };
    match with_kernel(|k| k.mutex_holder(m)) {
        Some(Ok(h)) => task_to_c(h),
        _ => core::ptr::null_mut(),
    }
}

/// `xQueueGetMutexHolderFromISR`.
#[no_mangle]
pub extern "C" fn xQueueGetMutexHolderFromISR(xSemaphore: *mut c_void) -> *mut c_void {
    let Some(m) = queue_from_c(xSemaphore) else {
        return core::ptr::null_mut();
    };
    match with_kernel(|k| k.mutex_holder_from_isr(m)) {
        Some(Ok(h)) => task_to_c(h),
        _ => core::ptr::null_mut(),
    }
}

/// `xQueueGiveMutexRecursive`.
#[no_mangle]
pub extern "C" fn xQueueGiveMutexRecursive(xMutex: *mut c_void) -> BaseType_t {
    let Some(m) = queue_from_c(xMutex) else {
        return PD_FAIL;
    };
    match with_kernel(|k| k.mutex_give_recursive(m)) {
        Some(Ok(())) => {
            crate::yield_now();
            PD_PASS
        }
        _ => PD_FAIL,
    }
}

/// `xQueueTakeMutexRecursive`.
#[no_mangle]
pub extern "C" fn xQueueTakeMutexRecursive(
    xMutex: *mut c_void,
    xTicksToWait: TickType_t,
) -> BaseType_t {
    let Some(m) = queue_from_c(xMutex) else {
        return PD_FAIL;
    };
    let ticks = u64::from(xTicksToWait);
    let mut spins = 0u32;
    loop {
        match with_kernel(|k| k.mutex_take_recursive(m, ticks)) {
            Some(Ok(Wait::Ready(()))) => return PD_PASS,
            Some(Ok(Wait::Blocked)) => {
                spin_guard(&mut spins, "xQueueTakeMutexRecursive");
                if ticks == 0 {
                    return PD_FAIL;
                }
                crate::yield_now();
            }
            _ => return PD_FAIL,
        }
    }
}

/// `xQueueCreateSet`.
#[no_mangle]
pub extern "C" fn xQueueCreateSet(uxEventQueueLength: UBaseType_t) -> *mut c_void {
    match with_kernel(|k| k.queue_create_set(uxEventQueueLength as usize)) {
        Some(Ok(h)) => {
            let i = usize::from(h.index());
            // A set is a queue of handles, so it has an item size like any
            // other queue -- and without one every `xQueuePeek` on it
            // copied ZERO bytes and reported `pdPASS`, leaving the caller's
            // handle at whatever it already held.
            if let Some(size) = ITEM_SIZES.get(i) {
                size.store(SET_ITEM_BYTES, Ordering::SeqCst);
            }
            if let Some(flag) = IS_SET.get(i) {
                flag.store(true, Ordering::SeqCst);
            }
            QUEUES_MADE.fetch_add(1, Ordering::Relaxed);
            queue_to_c(h)
        }
        _ => core::ptr::null_mut(),
    }
}

/// `xQueueAddToSet`.
#[no_mangle]
pub extern "C" fn xQueueAddToSet(
    xQueueOrSemaphore: *mut c_void,
    xQueueSet: *mut c_void,
) -> BaseType_t {
    let (Some(q), Some(set)) = (queue_from_c(xQueueOrSemaphore), queue_from_c(xQueueSet)) else {
        return PD_FAIL;
    };
    match with_kernel(|k| k.queue_add_to_set(q, set)) {
        Some(Ok(true)) => PD_PASS,
        _ => PD_FAIL,
    }
}

/// `xQueueRemoveFromSet`.
#[no_mangle]
pub extern "C" fn xQueueRemoveFromSet(
    xQueueOrSemaphore: *mut c_void,
    xQueueSet: *mut c_void,
) -> BaseType_t {
    let (Some(q), Some(set)) = (queue_from_c(xQueueOrSemaphore), queue_from_c(xQueueSet)) else {
        return PD_FAIL;
    };
    match with_kernel(|k| k.queue_remove_from_set(q, set)) {
        Some(Ok(true)) => PD_PASS,
        _ => PD_FAIL,
    }
}

/// `xQueueSelectFromSet`.
#[no_mangle]
pub extern "C" fn xQueueSelectFromSet(
    xQueueSet: *mut c_void,
    xTicksToWait: TickType_t,
) -> *mut c_void {
    let Some(set) = queue_from_c(xQueueSet) else {
        return core::ptr::null_mut();
    };
    let ticks = u64::from(xTicksToWait);
    let mut spins = 0u32;
    loop {
        match with_kernel(|k| k.queue_select_from_set(set, ticks)) {
            Some(Ok(Wait::Ready(Some(q)))) => return queue_to_c(q),
            Some(Ok(Wait::Ready(None))) => return core::ptr::null_mut(),
            Some(Ok(Wait::Blocked)) => {
                spin_guard(&mut spins, "xQueueSelectFromSet");
                if ticks == 0 {
                    return core::ptr::null_mut();
                }
                crate::yield_now();
            }
            _ => return core::ptr::null_mut(),
        }
    }
}

/// `xQueueSelectFromSetFromISR`.
#[no_mangle]
pub extern "C" fn xQueueSelectFromSetFromISR(xQueueSet: *mut c_void) -> *mut c_void {
    let Some(set) = queue_from_c(xQueueSet) else {
        return core::ptr::null_mut();
    };
    match with_kernel(|k| k.queue_select_from_set(set, 0)) {
        Some(Ok(Wait::Ready(Some(q)))) => queue_to_c(q),
        _ => core::ptr::null_mut(),
    }
}

// ===================================================== the event groups ==

/// `xEventGroupCreate`.
#[no_mangle]
pub extern "C" fn xEventGroupCreate() -> *mut c_void {
    match with_kernel(|k| k.event_group_create()) {
        Some(Ok(h)) => {
            EG_MADE.fetch_add(1, Ordering::Relaxed);
            group_to_c(h)
        }
        _ => core::ptr::null_mut(),
    }
}

/// `vEventGroupDelete`.
#[no_mangle]
pub extern "C" fn vEventGroupDelete(xEventGroup: *mut c_void) {
    if let Some(g) = group_from_c(xEventGroup) {
        let _ = with_kernel(|k| k.event_group_delete(g));
    }
}

/// `xEventGroupSetBits`.
#[no_mangle]
pub extern "C" fn xEventGroupSetBits(xEventGroup: *mut c_void, uxBitsToSet: u32) -> u32 {
    let Some(g) = group_from_c(xEventGroup) else {
        return 0;
    };
    let bits = with_kernel(|k| k.event_group_set_bits(g, uxBitsToSet));
    crate::yield_now();
    match bits {
        Some(Ok(b)) => b,
        _ => 0,
    }
}

/// `xEventGroupClearBits`.
#[no_mangle]
pub extern "C" fn xEventGroupClearBits(xEventGroup: *mut c_void, uxBitsToClear: u32) -> u32 {
    let Some(g) = group_from_c(xEventGroup) else {
        return 0;
    };
    match with_kernel(|k| k.event_group_clear_bits(g, uxBitsToClear)) {
        Some(Ok(b)) => b,
        _ => 0,
    }
}

/// `xEventGroupWaitBits`.
#[no_mangle]
pub extern "C" fn xEventGroupWaitBits(
    xEventGroup: *mut c_void,
    uxBitsToWaitFor: u32,
    xClearOnExit: BaseType_t,
    xWaitForAllBits: BaseType_t,
    xTicksToWait: TickType_t,
) -> u32 {
    let Some(g) = group_from_c(xEventGroup) else {
        return 0;
    };
    let ticks = u64::from(xTicksToWait);
    let mut spins = 0u32;
    loop {
        match with_kernel(|k| {
            k.event_group_wait_bits(
                g,
                uxBitsToWaitFor,
                xClearOnExit != 0,
                xWaitForAllBits != 0,
                ticks,
            )
        }) {
            Some(Ok(Wait::Ready(bits))) => {
                EG_WAIT_OK.fetch_add(1, Ordering::Relaxed);
                return bits;
            }
            Some(Ok(Wait::Blocked)) => {
                spin_guard(&mut spins, "xEventGroupWaitBits");
                if ticks == 0 {
                    EG_WAIT_FAIL.fetch_add(1, Ordering::Relaxed);
                    return 0;
                }
                crate::yield_now();
            }
            _ => {
                EG_WAIT_FAIL.fetch_add(1, Ordering::Relaxed);
                return 0;
            }
        }
    }
}

/// `xEventGroupSync`.
#[no_mangle]
pub extern "C" fn xEventGroupSync(
    xEventGroup: *mut c_void,
    uxBitsToSet: u32,
    uxBitsToWaitFor: u32,
    xTicksToWait: TickType_t,
) -> u32 {
    let Some(g) = group_from_c(xEventGroup) else {
        return 0;
    };
    let ticks = u64::from(xTicksToWait);
    let mut spins = 0u32;
    loop {
        match with_kernel(|k| k.event_group_sync(g, uxBitsToSet, uxBitsToWaitFor, ticks)) {
            Some(Ok(Wait::Ready(bits))) => return bits,
            Some(Ok(Wait::Blocked)) => {
                spin_guard(&mut spins, "xEventGroupSync");
                if ticks == 0 {
                    return 0;
                }
                crate::yield_now();
            }
            _ => return 0,
        }
    }
}

/// `xEventGroupGetBitsFromISR`.
#[no_mangle]
pub extern "C" fn xEventGroupGetBitsFromISR(xEventGroup: *mut c_void) -> u32 {
    let Some(g) = group_from_c(xEventGroup) else {
        return 0;
    };
    EG_GETISR_CALLS.fetch_add(1, Ordering::Relaxed);
    match with_kernel(|k| k.event_group_bits_from_isr(g)) {
        Some(Ok(b)) => {
            if b != 0 {
                EG_GETISR_NONZERO.fetch_add(1, Ordering::Relaxed);
            }
            b
        }
        _ => {
            EG_GETISR_ERR.fetch_add(1, Ordering::Relaxed);
            0
        }
    }
}

/// `xEventGroupSetBitsFromISR`.
#[no_mangle]
pub unsafe extern "C" fn xEventGroupSetBitsFromISR(
    xEventGroup: *mut c_void,
    uxBitsToSet: u32,
    pxHigherPriorityTaskWoken: *mut BaseType_t,
) -> BaseType_t {
    let Some(g) = group_from_c(xEventGroup) else {
        return PD_FAIL;
    };
    match with_kernel(|k| k.event_group_set_bits_from_isr(g, uxBitsToSet)) {
        Some(Ok((_, woken))) => {
            // SAFETY: the caller's own flag, if it passed one.
            unsafe { set_woken(pxHigherPriorityTaskWoken, woken) };
            EG_SETISR_OK.fetch_add(1, Ordering::Relaxed);
            PD_PASS
        }
        _ => {
            EG_SETISR_FAIL.fetch_add(1, Ordering::Relaxed);
            PD_FAIL
        }
    }
}

/// `xEventGroupClearBitsFromISR`.
#[no_mangle]
pub extern "C" fn xEventGroupClearBitsFromISR(
    xEventGroup: *mut c_void,
    uxBitsToClear: u32,
) -> BaseType_t {
    let Some(g) = group_from_c(xEventGroup) else {
        return PD_FAIL;
    };
    match with_kernel(|k| k.event_group_clear_bits_from_isr(g, uxBitsToClear)) {
        Some(Ok(_)) => PD_PASS,
        _ => PD_FAIL,
    }
}

// ==================================================== the stream buffers ==

/// `xStreamBufferGenericCreate`. Message buffers are stream buffers with a
/// length prefix, distinguished by `xIsMessageBuffer`.
#[no_mangle]
pub extern "C" fn xStreamBufferGenericCreate(
    xBufferSizeBytes: usize,
    xTriggerLevelBytes: usize,
    xIsMessageBuffer: BaseType_t,
    _pxSendCompletedCallback: *mut c_void,
    _pxReceiveCompletedCallback: *mut c_void,
) -> *mut c_void {
    let made = if xIsMessageBuffer != 0 {
        with_kernel(|k| k.message_buffer_create(xBufferSizeBytes))
    } else {
        with_kernel(|k| k.stream_buffer_create(xBufferSizeBytes, xTriggerLevelBytes))
    };
    match made {
        Some(Ok(h)) => {
            BUFFERS_MADE.fetch_add(1, Ordering::Relaxed);
            if let Some(flag) = IS_TRIGGER_BUFFER.get(usize::from(h.index())) {
                flag.store(xBufferSizeBytes == TRIGGER_TEST_BYTES, Ordering::SeqCst);
            }
            stream_to_c(h)
        }
        _ => {
            // A refusal here is SILENT to the C: the demo carries on
            // without its buffer, exactly as real FreeRTOS would, and the
            // stall appears somewhere else entirely.
            crate::note!(
                "CAPI xStreamBufferCreate REFUSED: {} bytes (arena exhausted?)",
                xBufferSizeBytes
            );
            core::ptr::null_mut()
        }
    }
}

/// `vStreamBufferDelete`.
#[no_mangle]
pub extern "C" fn vStreamBufferDelete(xStreamBuffer: *mut c_void) {
    BUFFERS_DELETED.fetch_add(1, Ordering::Relaxed);
    if let Some(b) = stream_from_c(xStreamBuffer) {
        let _ = with_kernel(|k| k.stream_buffer_delete(b));
    }
}

/// `xStreamBufferSend`.
#[no_mangle]
pub unsafe extern "C" fn xStreamBufferSend(
    xStreamBuffer: *mut c_void,
    pvTxData: *const c_void,
    xDataLengthBytes: usize,
    xTicksToWait: TickType_t,
) -> usize {
    let Some(b) = stream_from_c(xStreamBuffer) else {
        return 0;
    };
    // SAFETY: the C promised `xDataLengthBytes` readable bytes.
    let data = unsafe { core::slice::from_raw_parts(pvTxData.cast::<u8>(), xDataLengthBytes) };
    let ticks = u64::from(xTicksToWait);
    let mut spins = 0u32;
    loop {
        match with_kernel(|k| k.stream_buffer_send(b, data, ticks)) {
            Some(Ok(Wait::Ready(n))) => return n,
            Some(Ok(Wait::Blocked)) => {
                spin_guard(&mut spins, "xStreamBufferSend");
                if ticks == 0 {
                    return 0;
                }
                crate::yield_now();
            }
            _ => return 0,
        }
    }
}

/// `xStreamBufferReceive`.
#[no_mangle]
pub unsafe extern "C" fn xStreamBufferReceive(
    xStreamBuffer: *mut c_void,
    pvRxData: *mut c_void,
    xBufferLengthBytes: usize,
    xTicksToWait: TickType_t,
) -> usize {
    let Some(b) = stream_from_c(xStreamBuffer) else {
        return 0;
    };
    // SAFETY: the C promised `xBufferLengthBytes` writable bytes.
    let out = unsafe { core::slice::from_raw_parts_mut(pvRxData.cast::<u8>(), xBufferLengthBytes) };
    let ticks = u64::from(xTicksToWait);
    let mut spins = 0u32;
    if ticks > 0 {
        SB_BLOCKING_RECVS.fetch_add(1, Ordering::Relaxed);
    }
    // The entry tick is sampled INSIDE the same critical section as the
    // first attempt, and the exit tick inside the successful one.
    //
    // Reading it in a section of its own looks harmless and is not: a tick
    // landing between the two sections is counted as time the receive
    // spent blocked, when the receive had not started. The window is a few
    // instructions -- about a tenth of a percent of a 20,000-cycle tick on
    // the M3, and vanishing on a host where a tick is three million -- and
    // "two of ~290 on the M3, zero on the host" is exactly that shape. An
    // instrument that reproduces the asymmetry it is measuring has to be
    // ruled out before the asymmetry is believed.
    let mut entered_at: Option<u64> = None;
    loop {
        let Some((now, outcome)) =
            with_kernel(|k| (k.tick_count(), k.stream_buffer_receive(b, out, ticks)))
        else {
            return 0;
        };
        if entered_at.is_none() {
            entered_at = Some(now);
        }
        match outcome {
            Ok(Wait::Ready(n)) => {
                if ticks > 0 {
                    if let Some(c) = SB_RECV_BYTES.get(n.min(9)) {
                        c.fetch_add(1, Ordering::Relaxed);
                    }
                    let trigger = IS_TRIGGER_BUFFER
                        .get(usize::from(b.index()))
                        .is_some_and(|f| f.load(Ordering::SeqCst));
                    if trigger {
                        if let Some(c) = SB_TRIGGER_BYTES.get(n.min(9)) {
                            c.fetch_add(1, Ordering::Relaxed);
                        }
                        let blocked = now.saturating_sub(entered_at.unwrap_or(now));
                        if let Some(c) = SB_TRIGGER_TICKS.get((blocked as usize).min(9)) {
                            c.fetch_add(1, Ordering::Relaxed);
                        }
                        // A receive that blocked longer than it asked.
                        if blocked > ticks {
                            let which = SB_OVERBLOCKS.fetch_add(1, Ordering::Relaxed) as usize;
                            if let Some(slot) = SB_OVERBLOCK_AT.get(which) {
                                slot.store(entered_at.unwrap_or(0) as u32, Ordering::Relaxed);
                            }
                        }
                    }
                }
                return n;
            }
            Ok(Wait::Blocked) => {
                spin_guard(&mut spins, "xStreamBufferReceive");
                if ticks == 0 {
                    return 0;
                }
                crate::yield_now();
            }
            Err(_) => return 0,
        }
    }
}

/// `xStreamBufferSendFromISR`.
#[no_mangle]
pub unsafe extern "C" fn xStreamBufferSendFromISR(
    xStreamBuffer: *mut c_void,
    pvTxData: *const c_void,
    xDataLengthBytes: usize,
    pxHigherPriorityTaskWoken: *mut BaseType_t,
) -> usize {
    let Some(b) = stream_from_c(xStreamBuffer) else {
        return 0;
    };
    // SAFETY: the C promised `xDataLengthBytes` readable bytes.
    let data = unsafe { core::slice::from_raw_parts(pvTxData.cast::<u8>(), xDataLengthBytes) };
    SB_ISR_SENDS.fetch_add(1, Ordering::Relaxed);
    match with_kernel(|k| k.stream_buffer_send_from_isr(b, data)) {
        Some(Ok((n, woken))) => {
            // SAFETY: the caller's own flag, if it passed one.
            unsafe { set_woken(pxHigherPriorityTaskWoken, woken) };
            n
        }
        _ => 0,
    }
}

/// `xStreamBufferReceiveFromISR`.
#[no_mangle]
pub unsafe extern "C" fn xStreamBufferReceiveFromISR(
    xStreamBuffer: *mut c_void,
    pvRxData: *mut c_void,
    xBufferLengthBytes: usize,
    pxHigherPriorityTaskWoken: *mut BaseType_t,
) -> usize {
    let Some(b) = stream_from_c(xStreamBuffer) else {
        return 0;
    };
    // SAFETY: the C promised `xBufferLengthBytes` writable bytes.
    let out = unsafe { core::slice::from_raw_parts_mut(pvRxData.cast::<u8>(), xBufferLengthBytes) };
    match with_kernel(|k| k.stream_buffer_receive_from_isr(b, out)) {
        Some(Ok((n, woken))) => {
            // SAFETY: the caller's own flag, if it passed one.
            unsafe { set_woken(pxHigherPriorityTaskWoken, woken) };
            n
        }
        _ => 0,
    }
}

/// `xStreamBufferBytesAvailable`.
#[no_mangle]
pub extern "C" fn xStreamBufferBytesAvailable(xStreamBuffer: *mut c_void) -> usize {
    let Some(b) = stream_from_c(xStreamBuffer) else {
        return 0;
    };
    match with_kernel(|k| k.stream_buffer_bytes_available(b)) {
        Some(Ok(n)) => n,
        _ => 0,
    }
}

/// `xStreamBufferSpacesAvailable`.
#[no_mangle]
pub extern "C" fn xStreamBufferSpacesAvailable(xStreamBuffer: *mut c_void) -> usize {
    let Some(b) = stream_from_c(xStreamBuffer) else {
        return 0;
    };
    match with_kernel(|k| k.stream_buffer_spaces_available(b)) {
        Some(Ok(n)) => n,
        _ => 0,
    }
}

/// `xStreamBufferIsEmpty`.
#[no_mangle]
pub extern "C" fn xStreamBufferIsEmpty(xStreamBuffer: *mut c_void) -> BaseType_t {
    let Some(b) = stream_from_c(xStreamBuffer) else {
        return PD_PASS;
    };
    match with_kernel(|k| k.stream_buffer_is_empty(b)) {
        Some(Ok(true)) => PD_PASS,
        _ => PD_FAIL,
    }
}

/// `xStreamBufferIsFull`.
#[no_mangle]
pub extern "C" fn xStreamBufferIsFull(xStreamBuffer: *mut c_void) -> BaseType_t {
    let Some(b) = stream_from_c(xStreamBuffer) else {
        return PD_FAIL;
    };
    match with_kernel(|k| k.stream_buffer_is_full(b)) {
        Some(Ok(true)) => PD_PASS,
        _ => PD_FAIL,
    }
}

/// `xStreamBufferReset`.
#[no_mangle]
pub extern "C" fn xStreamBufferReset(xStreamBuffer: *mut c_void) -> BaseType_t {
    let Some(b) = stream_from_c(xStreamBuffer) else {
        return PD_FAIL;
    };
    match with_kernel(|k| k.stream_buffer_reset(b)) {
        Some(Ok(true)) => PD_PASS,
        _ => PD_FAIL,
    }
}

/// `xStreamBufferNextMessageLengthBytes`.
#[no_mangle]
pub extern "C" fn xStreamBufferNextMessageLengthBytes(xStreamBuffer: *mut c_void) -> usize {
    let Some(b) = stream_from_c(xStreamBuffer) else {
        return 0;
    };
    match with_kernel(|k| k.stream_buffer_next_message_length(b)) {
        Some(Ok(n)) => n,
        _ => 0,
    }
}

/// `xStreamBufferSendCompletedFromISR`.
#[no_mangle]
pub unsafe extern "C" fn xStreamBufferSendCompletedFromISR(
    xStreamBuffer: *mut c_void,
    pxHigherPriorityTaskWoken: *mut BaseType_t,
) -> BaseType_t {
    let Some(b) = stream_from_c(xStreamBuffer) else {
        return PD_FAIL;
    };
    match with_kernel(|k| k.send_completed_from_isr(b)) {
        Some(Ok(woken)) => {
            // SAFETY: the caller's own flag, if it passed one.
            unsafe { set_woken(pxHigherPriorityTaskWoken, woken) };
            PD_PASS
        }
        _ => PD_FAIL,
    }
}

// =========================================================== the timers ==

/// The C callback for each timer, by timer index.
///
/// The kernel's `timer_create` takes a `u16` callback INDEX, not a function
/// pointer — handles are indices there too. The C hands us a pointer, so
/// the seam keeps the table and passes the index across.
#[allow(clippy::declare_interior_mutable_const)]
/// A timer's `TimerCallbackFunction_t`, kept here because the kernel
/// cannot hold a pointer.
///
/// `AtomicUsize`, not `AtomicU32`: it IS a C function pointer, and the
/// same truncation that killed [`CEntry`] on a 64-bit host was sitting
/// here too, behind a comment of mine claiming it was "a small number".
/// It was not; only the M3 cell made that look true.
const NO_CB: AtomicUsize = AtomicUsize::new(0);
static TIMER_CALLBACKS: [AtomicUsize; crate::TIMERS] = [NO_CB; crate::TIMERS];

/// `configMAX_TASK_NAME_LEN` and one byte for the NUL.
///
/// Read from the cell's own `Config` rather than written down again: the
/// C's `configMAX_TASK_NAME_LEN` and this are two ends of one number, and
/// the ledger already records what a disagreement between two halves of
/// one configuration costs.
const NAME_BYTES: usize =
    <crate::CapiConfig as rusty_rtos_core::config::Config>::MAX_TASK_NAME_LEN + 1;

/// A timer's name, NUL-terminated, for `pcTimerGetName`.
///
/// **Why this lives here and not in the kernel.** The kernel already keeps
/// the name, in a fixed-width `Name` with a length — a Rust string. What a
/// C caller needs is a pointer to NUL-terminated bytes that stays valid
/// after the call returns, and building one out of a `Name` means either
/// pointing into a temporary (a dangling pointer) or giving the kernel a C
/// representation it has no other use for. FreeRTOS's own answer is a
/// `char pcTimerName[ configMAX_TASK_NAME_LEN ]` INSIDE the timer object,
/// valid for the timer's life; this is that array, held by the shim whose
/// job the C representation is.
///
/// `TimerDemo.c:304` is what asked for it: `configASSERT( strcmp(
/// pcTimerGetName( ... ), "FR Timer" ) == 0 )`. Returning `""` passed
/// every other demo in the corpus and failed on the first line of the one
/// that looks.
const NO_NAME: [AtomicU8; NAME_BYTES] = [const { AtomicU8::new(0) }; NAME_BYTES];
static TIMER_NAMES: [[AtomicU8; NAME_BYTES]; crate::TIMERS] = [NO_NAME; crate::TIMERS];

/// Record a timer's name at creation, truncating as the C's fixed-width
/// array does.
fn remember_timer_name(index: usize, name: &str) {
    let Some(buf) = TIMER_NAMES.get(index) else {
        return;
    };
    let bytes = name.as_bytes();
    for (i, slot) in buf.iter().enumerate() {
        // The last byte is ALWAYS the terminator, so a name that fills the
        // buffer is truncated rather than running off the end -- which is
        // what FreeRTOS's own `pcTimerName[]` does with a long name.
        let terminator = i.saturating_add(1) >= NAME_BYTES;
        let byte = if terminator {
            0
        } else {
            bytes.get(i).copied().unwrap_or(0)
        };
        slot.store(byte, Ordering::Relaxed);
    }
}

/// `void (*)( TimerHandle_t )`.
type TimerCallbackFunction_t = unsafe extern "C" fn(*mut c_void);

/// Timers that have expired and whose C callback has not run yet.
///
/// A ring rather than a flag per timer, because two expiries of the same
/// auto-reload timer are two callbacks and collapsing them would be a
/// quiet fidelity loss.
const EXPIRY_RING: usize = 64;
#[allow(clippy::declare_interior_mutable_const)]
const NO_EXPIRY: AtomicU32 = AtomicU32::new(0);
static EXPIRED: [AtomicU32; EXPIRY_RING] = [NO_EXPIRY; EXPIRY_RING];
static EXPIRY_HEAD: AtomicU32 = AtomicU32::new(0);
static EXPIRY_TAIL: AtomicU32 = AtomicU32::new(0);
/// Expiries dropped because the ring was full -- reported, not hidden.
pub static EXPIRIES_LOST: AtomicU32 = AtomicU32::new(0);

/// Record that a timer expired. Called from `TickHook::timer`.
///
/// It only records. The callback is C, and C calls back into the kernel --
/// but `TickHook::timer` is handed `&mut K`, so the kernel is already
/// borrowed when it runs and any `with_kernel` inside the callback would
/// be a second borrow of the same object. So the daemon task runs them
/// afterwards, outside the borrow, which is where the C runs them too.
pub fn note_timer_expiry(timer: rusty_rtos_core::handle::TimerHandle) {
    TIMER_EXPIRIES.fetch_add(1, Ordering::Relaxed);
    let head = EXPIRY_HEAD.load(Ordering::Relaxed);
    let tail = EXPIRY_TAIL.load(Ordering::Relaxed);
    if head.wrapping_sub(tail) as usize >= EXPIRY_RING {
        EXPIRIES_LOST.fetch_add(1, Ordering::Relaxed);
        return;
    }
    let slot = (head as usize) % EXPIRY_RING;
    if let Some(cell) = EXPIRED.get(slot) {
        cell.store(timer.to_raw(), Ordering::SeqCst);
        EXPIRY_HEAD.store(head.wrapping_add(1), Ordering::Relaxed);
    }
}

/// Run the C callbacks of every timer that has expired since last time.
///
/// Called from the timer daemon TASK, with no kernel borrow held, which is
/// what lets the callback use the whole API the way the C does.
pub fn drain_timer_expiries() {
    loop {
        let tail = EXPIRY_TAIL.load(Ordering::Relaxed);
        if tail == EXPIRY_HEAD.load(Ordering::Relaxed) {
            return;
        }
        let slot = (tail as usize) % EXPIRY_RING;
        let Some(cell) = EXPIRED.get(slot) else {
            return;
        };
        let raw = cell.load(Ordering::SeqCst);
        EXPIRY_TAIL.store(tail.wrapping_add(1), Ordering::Relaxed);

        let timer = rusty_rtos_core::handle::TimerHandle::from_raw(raw);
        let Some(entry) = TIMER_CALLBACKS.get(usize::from(timer.index())) else {
            continue;
        };
        let f = entry.load(Ordering::SeqCst);
        if f == 0 {
            continue;
        }
        // SAFETY: stored by `xTimerCreate` from a `TimerCallbackFunction_t`
        // the C passed, and read back only for the slot it was stored
        // against.
        let f: TimerCallbackFunction_t = unsafe { core::mem::transmute(f) };
        TIMER_CALLBACKS_RUN.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the C's own callback, given its own handle.
        unsafe { f(timer_to_c(timer)) };
    }
}

/// `xTimerCreate`.
#[no_mangle]
pub unsafe extern "C" fn xTimerCreate(
    pcTimerName: *const core::ffi::c_char,
    xTimerPeriodInTicks: TickType_t,
    xAutoReload: BaseType_t,
    pvTimerID: *mut c_void,
    pxCallbackFunction: *mut c_void,
) -> *mut c_void {
    let name = c_name(pcTimerName);
    let made = with_kernel(|k| {
        k.timer_create(
            name,
            u64::from(xTimerPeriodInTicks),
            xAutoReload != 0,
            pvTimerID as usize as u64,
            0,
        )
    });
    match made {
        Some(Ok(h)) => {
            TIMERS_MADE.fetch_add(1, Ordering::Relaxed);
            if let Some(slot) = TIMER_CALLBACKS.get(usize::from(h.index())) {
                slot.store(pxCallbackFunction as usize, Ordering::SeqCst);
            }
            remember_timer_name(usize::from(h.index()), name);
            timer_to_c(h)
        }
        _ => core::ptr::null_mut(),
    }
}

/// `xTimerGenericCommandFromTask`.
#[no_mangle]
pub extern "C" fn xTimerGenericCommandFromTask(
    xTimer: *mut c_void,
    xCommandID: BaseType_t,
    xOptionalValue: TickType_t,
    _pxHigherPriorityTaskWoken: *mut BaseType_t,
    xTicksToWait: TickType_t,
) -> BaseType_t {
    let Some(t) = timer_from_c(xTimer) else {
        return PD_FAIL;
    };
    let ticks = u64::from(xTicksToWait);
    let value = u64::from(xOptionalValue);
    let done = match xCommandID {
        // tmrCOMMAND_START / _RESET / _STOP / _CHANGE_PERIOD / _DELETE
        1 => {
            TIMERS_STARTED.fetch_add(1, Ordering::Relaxed);
            with_kernel(|k| k.timer_start(t, ticks))
        }
        2 => with_kernel(|k| k.timer_reset(t, ticks)),
        3 => with_kernel(|k| k.timer_stop(t, ticks)),
        4 => with_kernel(|k| k.timer_change_period(t, value, ticks)),
        5 => {
            TIMERS_DELETED.fetch_add(1, Ordering::Relaxed);
            with_kernel(|k| k.timer_delete(t, ticks))
        }
        _ => return PD_FAIL,
    };
    match done {
        Some(Ok(Wait::Ready(true))) => {
            TIMER_CMD_SENT.fetch_add(1, Ordering::Relaxed);
            PD_PASS
        }
        _ => {
            TIMER_CMD_REFUSED.fetch_add(1, Ordering::Relaxed);
            PD_FAIL
        }
    }
}

/// `xTimerGenericCommandFromISR`.
#[no_mangle]
pub unsafe extern "C" fn xTimerGenericCommandFromISR(
    xTimer: *mut c_void,
    xCommandID: BaseType_t,
    xOptionalValue: TickType_t,
    pxHigherPriorityTaskWoken: *mut BaseType_t,
    // `timers.h` passes this BY VALUE -- `const TickType_t xTicksToWait`.
    // It was declared here as a pointer, and because the argument is
    // unused the mistake was invisible: same register, same width, never
    // dereferenced. The header gate found it by compiling our declaration
    // beside the oracle's, which is the only place the two are ever
    // compared.
    _xTicksToWait: TickType_t,
) -> BaseType_t {
    let Some(t) = timer_from_c(xTimer) else {
        return PD_FAIL;
    };
    let value = u64::from(xOptionalValue);
    let done = match xCommandID {
        6 => with_kernel(|k| k.timer_start_from_isr(t)),
        7 => with_kernel(|k| k.timer_reset_from_isr(t)),
        8 => with_kernel(|k| k.timer_stop_from_isr(t)),
        9 => with_kernel(|k| k.timer_change_period_from_isr(t, value)),
        _ => return PD_FAIL,
    };
    match done {
        Some(Ok((true, woken))) => {
            // SAFETY: the caller's own flag, if it passed one.
            unsafe { set_woken(pxHigherPriorityTaskWoken, woken) };
            PD_PASS
        }
        _ => PD_FAIL,
    }
}

/// `xTimerIsTimerActive`.
#[no_mangle]
pub extern "C" fn xTimerIsTimerActive(xTimer: *mut c_void) -> BaseType_t {
    let Some(t) = timer_from_c(xTimer) else {
        return PD_FAIL;
    };
    match with_kernel(|k| k.timer_is_active(t)) {
        Some(Ok(true)) => PD_PASS,
        _ => PD_FAIL,
    }
}

/// `pvTimerGetTimerID`.
#[no_mangle]
pub extern "C" fn pvTimerGetTimerID(xTimer: *const c_void) -> *mut c_void {
    let Some(t) = timer_from_c(xTimer.cast_mut()) else {
        return core::ptr::null_mut();
    };
    match with_kernel(|k| k.timer_id(t)) {
        Some(Ok(id)) => id as usize as *mut c_void,
        _ => core::ptr::null_mut(),
    }
}

/// `vTimerSetTimerID`.
#[no_mangle]
pub extern "C" fn vTimerSetTimerID(xTimer: *mut c_void, pvNewID: *mut c_void) {
    if let Some(t) = timer_from_c(xTimer) {
        let _ = with_kernel(|k| k.timer_set_id(t, pvNewID as usize as u64));
    }
}

/// `pcTimerGetName`.
///
/// A pointer into [`TIMER_NAMES`], which is `static`, so it outlives this
/// call and every caller — the lifetime FreeRTOS's own version has, where
/// the buffer lives inside the timer object.
///
/// An unknown handle gets `""` rather than a null pointer, because the C
/// hands this straight to `strcmp` without checking.
#[no_mangle]
pub extern "C" fn pcTimerGetName(xTimer: *mut c_void) -> *const core::ffi::c_char {
    let Some(timer) = timer_from_c(xTimer) else {
        return c"".as_ptr();
    };
    let Some(first) = TIMER_NAMES
        .get(usize::from(timer.index()))
        .and_then(|buf| buf.first())
    else {
        return c"".as_ptr();
    };
    first.as_ptr().cast_const().cast::<core::ffi::c_char>()
}

/// `uxTimerGetReloadMode`.
#[no_mangle]
pub extern "C" fn uxTimerGetReloadMode(xTimer: *mut c_void) -> UBaseType_t {
    let Some(t) = timer_from_c(xTimer) else {
        return 0;
    };
    match with_kernel(|k| k.timer_auto_reload(t)) {
        Some(Ok(true)) => 1,
        _ => 0,
    }
}

/// `vTimerSetReloadMode`.
#[no_mangle]
pub extern "C" fn vTimerSetReloadMode(xTimer: *mut c_void, xAutoReload: BaseType_t) {
    // `timers.h` says `const BaseType_t xAutoReload`. This was
    // `UBaseType_t`, which is the same width and a different type -- so
    // the two declarations conflict even though no call could tell. The
    // header gate is what noticed.
    if let Some(t) = timer_from_c(xTimer) {
        let _ = with_kernel(|k| k.timer_set_auto_reload(t, xAutoReload != 0));
    }
}

// ========================================================== more plumbing ==

/// Set a C `pxHigherPriorityTaskWoken` flag, if the caller passed one.
///
/// # Safety
/// `flag` is either null or a writable `BaseType_t` the caller owns.
unsafe fn set_woken(flag: *mut BaseType_t, woken: rusty_rtos_core::isr::Woken) {
    if !woken.needed() {
        return;
    }
    // Remember it even when the caller did not ask.
    //
    // In the C, an ISR that wakes a higher-priority task ends with
    // `portYIELD_FROM_ISR( xHigherPriorityTaskWoken )` -- and a caller is
    // allowed to pass NULL for the flag and skip that, because on a chip
    // the pending switch is taken at the end of the interrupt anyway.
    //
    // `xNotifyTaskFromISR` in `TaskNotify.c` passes NULL, and our tick
    // handler asks `increment_tick` whether to switch BEFORE it runs the
    // demos' ISR halves. So a task woken by an ISR half had its wake-up
    // dropped on the floor: it stayed ready but unscheduled until
    // something else happened to yield. `TaskNotify` and
    // `TaskNotifyArray` both stall on exactly that, on BOTH ports, and
    // only a sustained check notices -- a single check at the end of a
    // 3,000-tick run sees a counter that moved early and stopped.
    ISR_WOKE.store(true, Ordering::Relaxed);
    if !flag.is_null() {
        // SAFETY: the caller's own variable.
        unsafe { *flag = PD_PASS };
    }
}

/// Set when an ISR-side call woke a task that outranks the running one.
///
/// This is `xHigherPriorityTaskWoken` for the callers that pass NULL: the
/// cell's tick handler ORs it into the switch decision after it has run
/// the demos' ISR halves, which is where `portYIELD_FROM_ISR` would be.
pub static ISR_WOKE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Take the flag, clearing it. `true` means a switch is owed.
pub fn take_isr_woke() -> bool {
    ISR_WOKE.swap(false, Ordering::Relaxed)
}

/// The C `void *` for a task handle.
///
/// A NULL kernel handle must come out as a NULL pointer, not as `0 + 1`.
/// `GenQTest.c` is what found this: it checks
/// `xSemaphoreGetMutexHolderFromISR( xMutex ) != NULL` after giving a mutex
/// back, and an unheld mutex answering `1` instead of NULL failed its
/// `configASSERT`. The `+ 1` exists to keep VALID handles non-null; it must
/// not manufacture a handle out of the absence of one.
fn task_to_c(handle: rusty_rtos_core::handle::TaskHandle) -> *mut c_void {
    handle_to_c(handle)
}

fn group_from_c(handle: *mut c_void) -> Option<rusty_rtos_core::handle::EventGroupHandle> {
    handle_from_c(handle)
}

fn group_to_c(handle: rusty_rtos_core::handle::EventGroupHandle) -> *mut c_void {
    handle_to_c(handle)
}

fn stream_from_c(handle: *mut c_void) -> Option<rusty_rtos_core::handle::StreamBufferHandle> {
    handle_from_c(handle)
}

fn stream_to_c(handle: rusty_rtos_core::handle::StreamBufferHandle) -> *mut c_void {
    handle_to_c(handle)
}

fn timer_from_c(handle: *mut c_void) -> Option<rusty_rtos_core::handle::TimerHandle> {
    handle_from_c(handle)
}

fn timer_to_c(handle: rusty_rtos_core::handle::TimerHandle) -> *mut c_void {
    handle_to_c(handle)
}

// ============================================================ tiny libc ==

/// `sprintf`, **only where there is no libc**, and only for the two
/// conversions the corpus actually uses.
///
/// # What the C asks for, measured rather than assumed
///
/// Every `sprintf` call in the whole of `Demo/Common/Minimal` is one of
/// two shapes: `sprintf( buf, "%d", (int) x )` in `MessageBufferDemo.c`
/// and `sprintf( buf, "%lu", (unsigned long) x )` in `MessageBufferAMP.c`.
/// One integer conversion, one argument, no width, no precision, no `%s`.
/// So this implements that and REFUSES anything else — a new call site gets
/// a negative return and an empty buffer rather than a plausible-looking
/// wrong string.
///
/// # The variadic argument, and why a fixed third parameter is correct here
///
/// `sprintf` is variadic and Rust cannot DEFINE a variadic function on
/// stable. It does not have to: on this target's AAPCS the first four words
/// of any call — variadic or not — arrive in `r0`–`r3`, so a call passing
/// `(buf, fmt, one 32-bit value)` puts them in `r0`, `r1`, `r2`, which is
/// exactly what a three-parameter `extern "C"` function reads. `int` and
/// `unsigned long` are both 32 bits on `thumbv7m`, so both call shapes land
/// identically.
///
/// **That is an assumption about the ABI, and it holds only for a single
/// 32-bit argument.** A call with two arguments, or a 64-bit one, would
/// read the wrong register — which is why the format parser refuses
/// everything it was not built for instead of trying.
#[cfg(target_os = "none")]
#[no_mangle]
pub unsafe extern "C" fn sprintf(
    out: *mut core::ffi::c_char,
    format: *const core::ffi::c_char,
    value: u32,
) -> i32 {
    if out.is_null() || format.is_null() {
        return -1;
    }
    let mut written: usize = 0;
    let mut read: usize = 0;
    // SAFETY: both pointers are non-null and the C owns NUL-terminated
    // storage at `format` and enough room at `out` for what it asked for.
    unsafe {
        loop {
            let ch = *format.add(read);
            if ch == 0 {
                break;
            }
            read = read.saturating_add(1);
            if ch != b'%' as core::ffi::c_char {
                *out.add(written) = ch;
                written = written.saturating_add(1);
                continue;
            }
            // A conversion. Skip a single `l` length modifier, which is the
            // only one `%lu` needs.
            let mut spec = *format.add(read);
            read = read.saturating_add(1);
            if spec == b'l' as core::ffi::c_char {
                spec = *format.add(read);
                read = read.saturating_add(1);
            }
            let signed = match spec {
                c if c == b'd' as core::ffi::c_char || c == b'i' as core::ffi::c_char => true,
                c if c == b'u' as core::ffi::c_char => false,
                // Anything else is a call this was not built for. Say so.
                _ => {
                    *out = 0;
                    return -1;
                }
            };
            written = write_u32(out, written, value, signed);
        }
        *out.add(written) = 0;
    }
    i32::try_from(written).unwrap_or(i32::MAX)
}

/// Write `value` as decimal at `out[at..]`, answering the new length.
#[cfg(target_os = "none")]
fn write_u32(out: *mut core::ffi::c_char, at: usize, value: u32, signed: bool) -> usize {
    // Ten digits holds `u32::MAX`; the sign is written separately.
    let mut digits = [0u8; 10];
    let mut count = 0usize;
    let negative = signed && (value as i32) < 0;
    // `unsigned_abs` rather than a negation, so `i32::MIN` is not a special
    // case that panics in debug and wraps in release.
    let mut rest = if negative {
        (value as i32).unsigned_abs()
    } else {
        value
    };
    loop {
        let digit = (rest % 10) as u8;
        if let Some(slot) = digits.get_mut(count) {
            *slot = b'0'.saturating_add(digit);
        }
        count = count.saturating_add(1);
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    let mut written = at;
    // SAFETY: the caller's buffer has room for what it asked to format;
    // `MessageBufferDemo.c` gives itself twelve bytes for a `%d`.
    unsafe {
        if negative {
            *out.add(written) = b'-' as core::ffi::c_char;
            written = written.saturating_add(1);
        }
        // Most significant first: the loop above produced them backwards.
        for index in (0..count).rev() {
            let digit = digits.get(index).copied().unwrap_or(b'0');
            *out.add(written) = digit as core::ffi::c_char;
            written = written.saturating_add(1);
        }
    }
    written
}

/// `strlen`, for the same reason as `strcmp`.
///
/// # Safety
/// The argument must be a NUL-terminated string the caller owns.
#[cfg(target_os = "none")]
#[no_mangle]
pub unsafe extern "C" fn strlen(s: *const core::ffi::c_char) -> usize {
    if s.is_null() {
        return 0;
    }
    let mut len = 0usize;
    // SAFETY: non-null and NUL-terminated, by the contract above.
    while unsafe { *s.add(len) } != 0 {
        len = len.saturating_add(1);
    }
    len
}

/// `fabs`, for `flop.c`, **only where there is no libm to link**.
///
/// Gated on `target_os = "none"` where `strcmp` above is not, and the
/// difference is deliberate rather than untidy. `strcmp` has exactly one
/// right answer and ours is it. `fabs` is a MATH function, and `flop.c`
/// compares its result against a tolerance of 0.001 — so where a platform
/// ships the reference implementation, that is the one the demo should be
/// judged against, not ours. On a bare-metal target there is no libm to
/// defer to, so this is it.
///
/// Implemented by clearing the sign bit rather than calling `f64::abs`,
/// which lives in `std` and not in `core`: on `no_std` the method that
/// looks free is a link error against libm.
#[cfg(target_os = "none")]
#[no_mangle]
pub extern "C" fn fabs(x: f64) -> f64 {
    f64::from_bits(x.to_bits() & !(1u64 << 63))
}

/// `strcmp`, which several demo files use to check a task's name.
///
/// Supplied here rather than linked from a C library because this cell has
/// no libc: the target has no bare-metal ARM sysroot on this box, and the
/// demos want exactly three string functions between them. Rust's
/// `compiler_builtins` already provides `memcpy`/`memset`/`memcmp`.
///
/// # Safety
/// Both arguments must be NUL-terminated strings the caller owns.
#[no_mangle]
pub unsafe extern "C" fn strcmp(a: *const core::ffi::c_char, b: *const core::ffi::c_char) -> i32 {
    let mut i = 0usize;
    loop {
        // SAFETY: both are NUL-terminated, so the walk stops at or before
        // the terminator of the shorter one.
        let (x, y) = unsafe { (*a.add(i), *b.add(i)) };
        if x != y {
            return i32::from(x as u8) - i32::from(y as u8);
        }
        if x == 0 {
            return 0;
        }
        i += 1;
    }
}

/// `strncmp`.
///
/// # Safety
/// Both arguments must be readable for `n` bytes or until a NUL.
#[no_mangle]
pub unsafe extern "C" fn strncmp(
    a: *const core::ffi::c_char,
    b: *const core::ffi::c_char,
    n: usize,
) -> i32 {
    let mut i = 0usize;
    while i < n {
        // SAFETY: as `strcmp`, bounded by `n`.
        let (x, y) = unsafe { (*a.add(i), *b.add(i)) };
        if x != y {
            return i32::from(x as u8) - i32::from(y as u8);
        }
        if x == 0 {
            return 0;
        }
        i += 1;
    }
    0
}
