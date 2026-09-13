// The names are the C's, exactly as in `seam/abi.rs`: `partest.h` and
// `serial.h` declare `uxLED`, `xValue`, `cOutChar`, and a Rust rename would
// make the two sides harder to compare for no gain.
#![allow(non_snake_case)]

//! The demo **project's** board drivers, as software.
//!
//! # Why these are not part of the ABI
//!
//! `partest.h` and `serial.h` are not kernel headers and none of these
//! symbols is a FreeRTOS API. They are what every FreeRTOS demo *project*
//! supplies for its own board — `ParTest.c` drives that board's LEDs,
//! `serial.c` its UART — and four demo files in `Demo/Common/Minimal` call
//! them. So they are NOT in `symbols.rs` and NOT in the generated
//! `kairos_capi.h`: adding them would claim the ABI exports things
//! FreeRTOS does not.
//!
//! That has a consequence worth saying out loud. Because they are not in
//! the generated header, the header gate does not audit their signatures
//! against the oracle's declarations the way it audits every real ABI
//! symbol. `capi/board_gate.c` closes that hole: it includes the oracle's
//! own `partest.h` and `serial.h` and takes the address of each of these,
//! so a signature that disagrees is a compile error in the same
//! translation unit rather than a wrong-width argument at run time.
//!
//! # The serial port is a LOOPBACK, because that is what the demo wants
//!
//! `comtest.c` says it plainly: "a loopback connector should be used so
//! that everything that is transmitted is received". On a board you wire
//! TX to RX. Here the wire is a kernel queue — so the driver is also a
//! CONSUMER of the thing under test, which is a small bonus: a queue bug
//! shows up as a serial failure.
//!
//! # The LEDs count, because two demo files have no checker
//!
//! `flash.c` and `flash_timer.c` export a start function and **nothing
//! else**: no `xAre...StillRunning`. There is no verdict to take from
//! them, and inventing one would be our opinion wearing the demo's
//! clothes. What they do is toggle LEDs on a schedule, so the toggle
//! counts are the only evidence they ran at all — reported as ours,
//! labelled as ours, and not counted in any N-of-N claim.

use core::ffi::{c_char, c_ulong, c_void};
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use rusty_rtos_capi_core::ctypes::{BaseType_t, TickType_t, UBaseType_t, PD_FAIL, PD_PASS};

// ------------------------------------------------------------------ LEDs --

/// `flash.c` drives three and `flash_timer.c` as many as it is told;
/// `comtest.c` adds two above whatever base it is given. Eight is more
/// than any of them asks for, and an LED beyond the end is ignored rather
/// than wrapping onto another one's count.
const LEDS: usize = 8;

static LED_TOGGLES: [AtomicU32; LEDS] = [const { AtomicU32::new(0) }; LEDS];
static LED_SETS: [AtomicU32; LEDS] = [const { AtomicU32::new(0) }; LEDS];
/// The current state, so a toggle is a real flip rather than a counter.
static LED_ON: [AtomicU32; LEDS] = [const { AtomicU32::new(0) }; LEDS];

/// `vParTestInitialise`.
#[no_mangle]
pub extern "C" fn vParTestInitialise() {
    for led in &LED_ON {
        led.store(0, Ordering::Relaxed);
    }
}

/// `vParTestSetLED`.
#[no_mangle]
pub extern "C" fn vParTestSetLED(uxLED: UBaseType_t, xValue: BaseType_t) {
    let Ok(index) = usize::try_from(uxLED) else {
        return;
    };
    if let Some(led) = LED_ON.get(index) {
        led.store(u32::from(xValue != 0), Ordering::Relaxed);
    }
    if let Some(count) = LED_SETS.get(index) {
        count.fetch_add(1, Ordering::Relaxed);
    }
}

/// `vParTestToggleLED`.
#[no_mangle]
pub extern "C" fn vParTestToggleLED(uxLED: UBaseType_t) {
    let Ok(index) = usize::try_from(uxLED) else {
        return;
    };
    if let Some(led) = LED_ON.get(index) {
        // `fetch_xor` rather than load-then-store: `flash.c` runs one task
        // per LED and `comtest.c` toggles from a third, so two tasks can
        // reach the same LED and a read-modify-write would lose a flip.
        led.fetch_xor(1, Ordering::Relaxed);
    }
    if let Some(count) = LED_TOGGLES.get(index) {
        count.fetch_add(1, Ordering::Relaxed);
    }
}

/// How many times each LED was toggled and set, for the run report.
#[must_use]
pub fn led_activity(index: usize) -> (u32, u32) {
    let toggles = LED_TOGGLES
        .get(index)
        .map_or(0, |c| c.load(Ordering::Relaxed));
    let sets = LED_SETS.get(index).map_or(0, |c| c.load(Ordering::Relaxed));
    (toggles, sets)
}

/// The number of LEDs this stand-in has.
#[must_use]
pub const fn led_count() -> usize {
    LEDS
}

// ---------------------------------------------------------- the loopback --

/// The queue standing in for the wire, as the C's opaque handle.
///
/// Zero means "no port", which is what `xComPortHandle` being a `void *`
/// lets us say. The demos call `xSerialPortInitMinimal` once.
static PORT: AtomicUsize = AtomicUsize::new(0);

/// `xSerialPortInitMinimal`. The baud rate is ignored: there is no wire to
/// clock, and pretending otherwise would be a number nobody could check.
#[no_mangle]
pub extern "C" fn xSerialPortInitMinimal(
    _ulWantedBaud: c_ulong,
    uxQueueLength: UBaseType_t,
) -> *mut c_void {
    let length = usize::try_from(uxQueueLength).unwrap_or(0).max(1);
    let made = crate::with_kernel(|k| k.queue_create(length));
    match made {
        Some(Ok(q)) => {
            let handle = crate::abi::handle_to_c(q);
            PORT.store(handle as usize, Ordering::SeqCst);
            handle
        }
        _ => core::ptr::null_mut(),
    }
}

/// The queue behind a port handle, ignoring which handle the caller passed.
///
/// `comtest.c` keeps the handle `xSerialPortInitMinimal` returned and hands
/// it back; `comtest_strings.c` passes `NULL` in places. One port either
/// way, so the stored one is the answer and a null argument is not an
/// error.
fn port_queue() -> Option<rusty_rtos_core::handle::QueueHandle> {
    let raw = PORT.load(Ordering::SeqCst);
    if raw == 0 {
        return None;
    }
    crate::abi::handle_from_c(raw as *mut c_void)
}

/// `xSerialPutChar`: onto the wire.
#[no_mangle]
pub extern "C" fn xSerialPutChar(
    _pxPort: *mut c_void,
    cOutChar: c_char,
    xBlockTime: TickType_t,
) -> BaseType_t {
    let Some(q) = port_queue() else {
        return PD_FAIL;
    };
    // The C's `signed char` is one byte; widen through `u8` so a negative
    // value is the same eight bits coming back out rather than a sign
    // extension the receiver would have to undo.
    let value = u64::from(cOutChar as u8);
    let ticks = u64::from(xBlockTime);
    // The ABI's retry protocol, which this driver has to follow exactly as
    // `xQueueGenericSend` does: `Blocked` is not failure, it is "call again
    // when this task next runs". Reporting it to the C as failure is what
    // broke `comtest` -- see the module header.
    loop {
        match crate::with_kernel(|k| k.queue_send(q, value, ticks)) {
            Some(Ok(rusty_rtos_kernel_core::queue::Wait::Ready(()))) => return PD_PASS,
            Some(Ok(rusty_rtos_kernel_core::queue::Wait::Blocked)) => {
                if ticks == 0 {
                    // No block time asked for, so a full wire really is a
                    // refusal -- a transmit buffer with no room, which the
                    // demos handle.
                    return PD_FAIL;
                }
                crate::yield_now();
            }
            _ => return PD_FAIL,
        }
    }
}

/// `xSerialGetChar`: off the wire.
#[no_mangle]
pub unsafe extern "C" fn xSerialGetChar(
    _pxPort: *mut c_void,
    pcRxedChar: *mut c_char,
    xBlockTime: TickType_t,
) -> BaseType_t {
    let Some(q) = port_queue() else {
        return PD_FAIL;
    };
    if pcRxedChar.is_null() {
        return PD_FAIL;
    }
    let ticks = u64::from(xBlockTime);
    loop {
        match crate::with_kernel(|k| k.queue_receive(q, ticks)) {
            Some(Ok(rusty_rtos_kernel_core::queue::Wait::Ready(value))) => {
                // SAFETY: checked non-null above; the C owns one byte there.
                unsafe { *pcRxedChar = (value as u8) as c_char };
                return PD_PASS;
            }
            Some(Ok(rusty_rtos_kernel_core::queue::Wait::Blocked)) => {
                if ticks == 0 {
                    return PD_FAIL;
                }
                crate::yield_now();
            }
            _ => return PD_FAIL,
        }
    }
}

/// `vSerialPutString`.
///
/// Character at a time with no block time, which is what the real drivers
/// do when they have no DMA: the queue is the transmit buffer.
#[no_mangle]
pub unsafe extern "C" fn vSerialPutString(
    pxPort: *mut c_void,
    pcString: *const c_char,
    usStringLength: u16,
) {
    if pcString.is_null() {
        return;
    }
    for offset in 0..usize::from(usStringLength) {
        // SAFETY: the C promises `usStringLength` readable bytes.
        let ch = unsafe { *pcString.add(offset) };
        if xSerialPutChar(pxPort, ch, 0) != PD_PASS {
            // A full wire drops the rest, exactly as a driver with no room
            // in its transmit buffer does. `comtest_strings.c` tolerates
            // it; dropping silently is the honest behaviour, and the
            // queue's own full counter is where it shows up.
            return;
        }
    }
}
