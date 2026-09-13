//! How a Kairos handle crosses the C seam, and how a queue item does.
//!
//! Both are small pieces of arithmetic, and both were the source of real
//! defects that took a running system a long way from the call that caused
//! them. They live here, away from any pointer, so that they can be tested
//! -- which on the firmware side they never could be.

use rusty_rtos_core::handle::{Handle, Kind};

use crate::ctypes::UBaseType_t;

/// The handle codec: a kernel handle as the `void *` the C holds.
///
/// # Why `+ 1`
///
/// A Kairos handle is `(index, generation)` packed into a `u32`, and the
/// first task ever created is `(0, 0)` -- which is `0`, which to C is
/// `NULL`, which means failure. So a valid handle is offset by one.
///
/// Carrying the generation across the seam is the point: a handle the C
/// kept after the object was deleted decodes to a stale generation and is
/// REJECTED, rather than silently naming whatever now occupies the slot.
///
/// # Why NULL must survive
///
/// The offset must not manufacture a handle out of the absence of one.
/// `xSemaphoreGetMutexHolder` on an unheld mutex answers the kernel's NULL
/// handle, and `0 + 1 = 1` made that a non-NULL pointer --
/// `GenQTest.c:564` asserts exactly that it is NULL, and it was the C that
/// found this, not us.
#[must_use]
pub fn handle_to_c<K: Kind>(handle: Handle<K>) -> usize {
    if handle.is_null() {
        0
    } else {
        // `to_raw` is a `u32`, so on any target this crate builds for the
        // `+ 1` cannot overflow a `usize`. Saturating says so without
        // adding a branch the optimiser will keep.
        (handle.to_raw() as usize).saturating_add(1)
    }
}

/// The inverse of [`handle_to_c`]. `None` for NULL, which every C API
/// treats as "no object", and for anything that could not have come from
/// this codec.
#[must_use]
pub fn handle_from_c<K: Kind>(raw: usize) -> Option<Handle<K>> {
    // `checked_sub` rather than `- 1`: `raw` came across an FFI boundary
    // and the zero case is the only one already excluded.
    let value = u32::try_from(raw.checked_sub(1)?).ok()?;
    Some(Handle::<K>::from_raw(value))
}

/// The widest item this seam can carry, and why.
///
/// A Kairos queue slot holds a `u64`; a FreeRTOS queue copies an arbitrary
/// `uxItemSize`. Up to eight bytes the two are the same thing and the copy
/// is exact. Beyond that they are not, so a queue asking for more is
/// REFUSED at creation -- a queue that silently dropped four bytes of
/// every item would produce a demo that runs and is wrong, which is the
/// worst outcome available.
pub const MAX_ITEM_BYTES: UBaseType_t = 8;

/// Can a queue of this item size be represented at all?
#[must_use]
pub const fn item_size_fits(ux_item_size: UBaseType_t) -> bool {
    ux_item_size <= MAX_ITEM_BYTES
}

/// Read one queue item out of the C's buffer.
///
/// Little-endian and zero-extended, so a `uint32_t` written by the C and
/// read back as a `uint32_t` is the same number, and the unused high bytes
/// of the slot are zero rather than whatever was there before.
#[must_use]
pub fn item_from_bytes(src: &[u8]) -> u64 {
    let mut buf = [0u8; 8];
    let n = src.len().min(8);
    // `get` both sides, though `n` is a minimum of both lengths: the
    // house rule is that a slice index is never bare, and a codec is the
    // last place to make an exception for an argument that "obviously"
    // fits.
    if let (Some(dst), Some(src)) = (buf.get_mut(..n), src.get(..n)) {
        dst.copy_from_slice(src);
    }
    u64::from_le_bytes(buf)
}

/// Write one queue item into the C's buffer, copying exactly the bytes the
/// queue's `uxItemSize` claims and no more.
///
/// Returns how many bytes were written, which is the caller's check that
/// it sized the destination the way the queue was created.
pub fn item_to_bytes(value: u64, dst: &mut [u8]) -> usize {
    let buf = value.to_le_bytes();
    let n = dst.len().min(8);
    if let (Some(dst), Some(src)) = (dst.get_mut(..n), buf.get(..n)) {
        dst.copy_from_slice(src);
        return n;
    }
    0
}

/// A queue SET holds the handles of its members, so its items are the one
/// kind whose contents must be translated on the way out: the kernel
/// stores `Handle::to_raw()`, and every handle the C holds is
/// [`handle_to_c`] of that.
///
/// `xQueueSelectFromSet` always did this, because it returns a handle
/// directly. `xQueuePeek` and `xQueueReceive` did not, because they copy
/// bytes and had no way to know what the bytes meant -- so peeking a set
/// reported `pdPASS` and wrote a NULL handle. `QueueSet.c` peeks the set
/// and compares the answer against the member handle it was given.
#[must_use]
pub fn set_item_to_c(raw: u64) -> u64 {
    if raw == 0 {
        0
    } else {
        // The same offset `handle_to_c` applies, on the raw form.
        raw.saturating_add(1)
    }
}

/// The bytes one member handle occupies in a set, on a target whose
/// pointers are `P` bytes wide.
#[must_use]
pub const fn set_item_bytes(pointer_width: usize) -> UBaseType_t {
    pointer_width as UBaseType_t
}

#[cfg(test)]
// The house lint policy (H-15..H-18) bans panicking APIs and bare
// indexing on every path. A test IS the path where a panic is the
// report, so tests opt out per file, as every Kairos package does.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;
    use rusty_rtos_core::handle::{Queue, Task};

    type T = Handle<Task>;
    type Q = Handle<Queue>;

    #[test]
    fn a_valid_handle_is_never_null_to_c() {
        // The first task ever created is index 0 at generation 1: a
        // generation of zero IS the null handle, so the arena never mints
        // one. The offset is therefore DEFENSIVE rather than load-bearing
        // -- `to_raw` of a live handle already has its generation in the
        // high half and so is already non-zero. It is kept because the
        // seam must not depend on that, and `handle_from_c` is the only
        // place the two conventions have to agree.
        let first = T::from_parts(0, 1);
        assert!(!first.is_null());
        assert_ne!(first.to_raw(), 0);
        assert_ne!(handle_to_c(first), 0);
    }

    #[test]
    fn generation_zero_is_the_null_handle_at_every_index() {
        for index in [0u16, 1, 7, Handle::<Task>::MAX_INDEX] {
            assert!(T::from_parts(index, 0).is_null(), "index {index}");
        }
    }

    /// K6 defect 1. `0 + 1 = 1` made the ABSENCE of a handle into a
    /// non-null pointer, and `GenQTest.c:564` asserted otherwise.
    #[test]
    fn a_null_handle_stays_null_across_the_seam() {
        assert_eq!(handle_to_c(T::NULL), 0);
        assert_eq!(handle_to_c(Q::NULL), 0);
        assert_eq!(handle_from_c::<Task>(0), None);
    }

    #[test]
    fn every_handle_round_trips() {
        for index in [0u16, 1, 2, 95, 1000, Handle::<Task>::MAX_INDEX] {
            // Generation 1 upward: zero is the null handle, which round
            // trips through `None` and is covered by its own test.
            for generation in [1u16, 7, u16::MAX] {
                let h = T::from_parts(index, generation);
                let c = handle_to_c(h);
                assert_eq!(handle_from_c::<Task>(c), Some(h), "{index}/{generation}");
            }
        }
    }

    #[test]
    fn the_codec_carries_the_generation_so_a_stale_handle_is_distinguishable() {
        let live = T::from_parts(4, 1);
        let stale = T::from_parts(4, 0);
        assert_ne!(handle_to_c(live), handle_to_c(stale));
    }

    #[test]
    fn an_item_round_trips_at_every_width_the_seam_allows() {
        for width in 1..=8usize {
            let mut buf = [0u8; 8];
            let value = 0x0123_4567_89ab_cdefu64 & (u64::MAX >> (64 - width * 8));
            assert_eq!(item_to_bytes(value, &mut buf[..width]), width);
            assert_eq!(item_from_bytes(&buf[..width]), value);
        }
    }

    #[test]
    fn a_narrow_read_zero_extends_rather_than_keeping_stale_high_bytes() {
        let four = [0xefu8, 0xcd, 0xab, 0x89];
        assert_eq!(item_from_bytes(&four), 0x89ab_cdef);
    }

    #[test]
    fn a_wider_item_than_the_slot_is_refused_not_truncated() {
        assert!(item_size_fits(8));
        assert!(!item_size_fits(9));
        assert!(!item_size_fits(12));
    }

    #[test]
    fn a_write_never_runs_past_the_c_s_buffer() {
        let mut dst = [0xAAu8; 4];
        assert_eq!(item_to_bytes(u64::MAX, &mut dst), 4);
        assert_eq!(dst, [0xff; 4]);
    }

    /// K6 defect 5. A set's items are handles and were being copied raw,
    /// so `xQueuePeek` on a set answered a handle the C had never seen.
    #[test]
    fn a_set_item_carries_the_same_offset_as_any_other_handle() {
        let member = Q::from_parts(5, 2);
        assert!(!member.is_null());
        let raw = u64::from(member.to_raw());
        assert_eq!(set_item_to_c(raw) as usize, handle_to_c(member));
    }

    #[test]
    fn an_empty_set_slot_stays_null() {
        assert_eq!(set_item_to_c(0), 0);
    }
}
