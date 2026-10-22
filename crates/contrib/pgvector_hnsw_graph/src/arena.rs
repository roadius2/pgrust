//! Append-only element storage addressed by u32 handles, the role of C's
//! relptr into the graph's memory (hnsw.h:144-152). Segment k holds 1024 << k
//! slots, so a handle never moves once published and no segment is ever
//! reallocated; slots and segments are pgsync OnceLocks, set once and read
//! without a lock.

use pgsync::OnceLock;

const BASE_SHIFT: u32 = 10;
const BASE: u64 = 1 << BASE_SHIFT;
/// Segments 0..=22 cover every u32 handle.
const SEGMENTS: usize = 23;

pub(crate) struct Arena<T> {
    segments: [OnceLock<Box<[OnceLock<T>]>>; SEGMENTS],
}

/// A handle's segment and its slot in that segment.
pub(crate) fn locate(id: u32) -> (usize, usize) {
    let n = id as u64 + BASE;
    let k = (63 - n.leading_zeros()) - BASE_SHIFT;
    (k as usize, (n - (BASE << k)) as usize)
}

impl<T> Arena<T> {
    pub(crate) fn new() -> Arena<T> {
        Arena { segments: std::array::from_fn(|_| OnceLock::new()) }
    }

    /// Store `value` at `id`. The caller hands out ids in sequence under the
    /// graph's allocator lock, so no two calls race for a slot or a segment.
    pub(crate) fn publish(&self, id: u32, value: T) {
        let (k, slot) = locate(id);
        if self.segments[k].get().is_none() {
            let fresh: Box<[OnceLock<T>]> = (0..(BASE << k)).map(|_| OnceLock::new()).collect();
            if self.segments[k].set(fresh).is_err() {
                unreachable!("hnsw graph segment {k} created twice");
            }
        }
        let segment = self.segments[k].get().expect("segment just created");
        if segment[slot].set(value).is_err() {
            unreachable!("hnsw graph element {id} stored twice");
        }
    }

    pub(crate) fn get(&self, id: u32) -> &T {
        let (k, slot) = locate(id);
        self.segments[k]
            .get()
            .and_then(|segment| segment[slot].get())
            .unwrap_or_else(|| panic!("hnsw graph element {id} is not allocated"))
    }
}
