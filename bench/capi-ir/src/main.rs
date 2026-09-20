//! Instruction counts for the C ABI's marshalling, which nothing measured
//! until now.
//!
//! This is the only SHIPPING surface in the family that had no instrument, and
//! every call a C application makes crosses it: a handle in, an item in or
//! out, a name in. `xQueueSend` marshals a handle and an item; `xTaskCreate`
//! marshals a handle and a name. The cost is paid per call, by every C user,
//! on top of the kernel work the call actually does.
//!
//! Four paths, and each is driven down BOTH arms:
//!
//! * **Handles.** Round-tripped to the C's `usize` and back, and then the
//!   values a C application can pass that are not handles at all -- NULL and
//!   a wild pointer -- because refusing those is the whole reason the
//!   conversion is fallible.
//! * **Queue items.** `item_from_bytes` and `item_to_bytes` at every width a
//!   `uxItemSize` can be, including the zero-size case a semaphore uses.
//! * **Names.** `strnlen` and `name_from` over terminated, UNTERMINATED and
//!   non-UTF-8 byte arrays. An unterminated name is what a C caller passing a
//!   fixed buffer produces, and it must stop at the limit rather than run.
//! * **The blocked decision**, which every non-blocking C call takes.
//!
//! A deterministic counter, not a clock. The verdict counts are the work
//! parity anchors: a change that moves any of them changed behaviour, and a
//! compiler that removed the work moves the checksum.

use rusty_rtos_capi_core::codec::{
    handle_from_c, handle_to_c, item_from_bytes, item_size_fits, item_to_bytes,
};
use rusty_rtos_capi_core::cstr::{name_from, strnlen};
use rusty_rtos_capi_core::retry::{Blocked, SpinGuard, on_blocked};
use rusty_rtos_capi_core::rtos_core::handle::{Handle, Queue, Task};

/// Enough repetitions that process startup is noise in the total.
const REPS: u32 = 30_000;

/// Raw `usize` values a C application can hand back, valid and not.
///
/// The invalid ones are the point: a C caller passing NULL or a stale pointer
/// is not a bug in this crate, it is the case this crate exists to refuse.
const RAWS: &[usize] = &[0, 1, 2, 0x0102, 0xFFFF, usize::MAX, 0x8000_0000, 42];

/// `uxItemSize` values, including zero -- which is what a semaphore is.
const ITEM_SIZES: &[usize] = &[0, 1, 2, 4, 8];

/// Names as a C caller supplies them: NUL-terminated, unterminated, empty,
/// and one that is not UTF-8 at all.
const NAMES: &[&[u8]] = &[
    b"IDLE\0\0\0\0",
    b"Tmr Svc\0",
    b"a\0bcdefg",
    b"unterminated",
    b"\0",
    b"",
    &[0xFF, 0xFE, b'x', 0, 0, 0, 0, 0],
    b"exactly8",
];

/// `configMAX_TASK_NAME_LEN`, as the demo configuration sets it.
const NAME_LIMIT: usize = 12;

fn main() {
    let mut handles = 0u64;
    let mut refused = 0u64;
    let mut items = 0u64;
    let mut names = 0u64;
    let mut gave_up = 0u64;
    let mut checksum = 0u64;

    for rep in 0..REPS {
        // ---- handles, both directions and both arms -----------------------
        for (i, raw) in RAWS.iter().enumerate() {
            match handle_from_c::<Task>(*raw) {
                Some(handle) => {
                    handles = handles.wrapping_add(1);
                    // Back out again: a round trip is what a C application
                    // actually does, and a conversion that lost a bit would
                    // show here rather than in either half alone.
                    let back = handle_to_c(handle);
                    checksum = checksum.wrapping_add(back as u64).wrapping_add(i as u64);
                }
                None => refused = refused.wrapping_add(1),
            }
            // The other kind, so a codec that confused two handle kinds is
            // not measured as if it had only one.
            if let Some(handle) = handle_from_c::<Queue>(*raw) {
                checksum = checksum.wrapping_add(handle_to_c(handle) as u64);
            }
        }

        // ---- queue items, every width ------------------------------------
        let mut scratch = [0u8; 8];
        for size in ITEM_SIZES {
            if !item_size_fits(*size as _) {
                continue;
            }
            let src = [
                rep as u8,
                0xA5,
                0x5A,
                (rep >> 8) as u8,
                1,
                2,
                3,
                4,
            ];
            if let Some(slice) = src.get(..*size) {
                let value = item_from_bytes(slice);
                let wrote = item_to_bytes(value, &mut scratch);
                items = items.wrapping_add(1);
                checksum = checksum.wrapping_add(value).wrapping_add(wrote as u64);
            }
        }

        // ---- names --------------------------------------------------------
        for name in NAMES {
            let len = strnlen(name, NAME_LIMIT);
            let text = name_from(name, NAME_LIMIT);
            names = names.wrapping_add(1);
            checksum = checksum
                .wrapping_add(len as u64)
                .wrapping_add(text.len() as u64);
        }

        // ---- the blocked decision, both arms ------------------------------
        for ticks in [0u64, 1, 100] {
            match on_blocked(ticks) {
                Blocked::GiveUp => gave_up = gave_up.wrapping_add(1),
                Blocked::Yield => checksum = checksum.wrapping_add(1),
            }
        }
        let mut guard = SpinGuard::with_limit(4);
        let mut spins = 0u64;
        while !guard.blocked() {
            spins = spins.wrapping_add(1);
        }
        checksum = checksum.wrapping_add(spins);
    }

    println!("checksum {checksum}");
    println!(
        "reps {REPS} handles {} refused {} items {} names {} gave_up {}",
        handles / u64::from(REPS),
        refused / u64::from(REPS),
        items / u64::from(REPS),
        names / u64::from(REPS),
        gave_up / u64::from(REPS)
    );
}
