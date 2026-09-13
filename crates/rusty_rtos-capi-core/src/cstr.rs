//! Reading a C string without reading past it.
//!
//! `pcName` arrives as a `const char *` and the seam has to turn it into a
//! `&str`. The pointer work belongs to whoever holds the pointer; the
//! RULE -- how far to look, what to do about a missing NUL, what to do
//! about bytes that are not UTF-8 -- is arithmetic, and it is here so that
//! it has tests.

/// `configMAX_TASK_NAME_LEN` is what the C copies; this is the ceiling the
/// seam will scan to regardless, so a caller that passes an unterminated
/// buffer is truncated rather than allowed to walk off the end of memory.
pub const MAX_NAME_BYTES: usize = 32;

/// A longer ceiling for a `__FILE__` path, which `configASSERT` reports.
/// Truncating a path at 32 characters hides which file the C is
/// complaining about, which is the only thing the message is for.
pub const MAX_PATH_BYTES: usize = 200;

/// How many bytes of `haystack` precede the first NUL, capped at `limit`.
///
/// This is the length a caller should then read; it never exceeds either
/// the cap or what it was shown.
#[must_use]
pub fn strnlen(haystack: &[u8], limit: usize) -> usize {
    let end = haystack.len().min(limit);
    match haystack
        .get(..end)
        .and_then(|h| h.iter().position(|&b| b == 0))
    {
        Some(n) => n,
        None => end,
    }
}

/// The name a C string denotes, truncated at `limit` and at its NUL.
///
/// Bytes that are not UTF-8 give `""` rather than an error: a task name is
/// a diagnostic, and refusing to create a task because its name was
/// Latin-1 would be a worse answer than an unnamed task.
#[must_use]
pub fn name_from(haystack: &[u8], limit: usize) -> &str {
    let n = strnlen(haystack, limit);
    haystack
        .get(..n)
        .and_then(|b| core::str::from_utf8(b).ok())
        .unwrap_or("")
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

    #[test]
    fn a_terminated_name_reads_up_to_its_nul() {
        assert_eq!(name_from(b"PollQ\0junk", MAX_NAME_BYTES), "PollQ");
    }

    #[test]
    fn an_unterminated_name_is_truncated_at_the_ceiling_not_read_past() {
        let wild = [b'x'; 64];
        assert_eq!(name_from(&wild, MAX_NAME_BYTES).len(), MAX_NAME_BYTES);
        assert_eq!(strnlen(&wild, MAX_NAME_BYTES), MAX_NAME_BYTES);
    }

    #[test]
    fn a_buffer_shorter_than_the_ceiling_bounds_the_scan() {
        assert_eq!(strnlen(b"ab", MAX_NAME_BYTES), 2);
        assert_eq!(name_from(b"ab", MAX_NAME_BYTES), "ab");
    }

    #[test]
    fn an_empty_name_is_empty_not_an_error() {
        assert_eq!(name_from(b"\0", MAX_NAME_BYTES), "");
        assert_eq!(name_from(b"", MAX_NAME_BYTES), "");
    }

    #[test]
    fn non_utf8_gives_no_name_rather_than_refusing_the_task() {
        assert_eq!(name_from(b"\xff\xfe\0", MAX_NAME_BYTES), "");
    }

    #[test]
    fn a_path_gets_a_longer_ceiling_than_a_task_name() {
        const { assert!(MAX_PATH_BYTES > MAX_NAME_BYTES) };
        let path = b"F:/coding/rusty_RTOS/oracle/FreeRTOS/Demo/Common/Minimal/QueueSet.c\0";
        assert!(name_from(path, MAX_PATH_BYTES).ends_with("QueueSet.c"));
        assert!(!name_from(path, MAX_NAME_BYTES).ends_with("QueueSet.c"));
    }
}
