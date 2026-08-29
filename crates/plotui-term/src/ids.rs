//! Process-wide Kitty image id allocation.

use std::sync::atomic::{AtomicU32, Ordering};

static NEXT_IMAGE_ID: AtomicU32 = AtomicU32::new(plotui_protocol::DEFAULT_IMAGE_ID);

/// Allocate a fresh Kitty image id. The first call returns
/// [`plotui_protocol::DEFAULT_IMAGE_ID`] — a single-plot app stays
/// bit-compatible with the fixed-id encoders — and each later call gets the
/// next id, so multiple plots in one process never clobber each other's
/// images. Ids stay well below 0xFFFFFF, keeping placeholder cells at three
/// characters (no "extra" diacritic).
pub fn next_image_id() -> u32 {
    NEXT_IMAGE_ID.fetch_add(1, Ordering::Relaxed)
}
