//! POSIX shared-memory transport to Unreal.
//!
//! The receiver publishes each latched frame into a shared region; a UE C++
//! component `mmap`s the same region and uploads it to a texture (README §6.1).
//! Spout is Windows-only and NDI/Syphon add codec/latency overhead we don't need
//! for a ~168 KB buffer, so we use plain shared memory with a seqlock for
//! tear-free single-writer/single-reader handoff.
//!
//! ## Region layout (little-endian, native)
//!
//! ```text
//!   off  size  field
//!     0     4  magic     = 0x315A5650 ("PVZ1")
//!     4     4  version   = 1
//!     8     4  num_voxels
//!    12     4  width      (cube 0 / world width)
//!    16     4  height
//!    20     4  length
//!    24     4  cube_count
//!    28     4  (reserved)
//!    32     8  seq        (u64 seqlock; even = stable, odd = writer in progress)
//!    64   N*3  RGB voxel data, (z,y,x) order, cubes concatenated
//! ```
//!
//! ### Reader protocol (UE side)
//! ```text
//!   loop:
//!     s1 = atomic_load(seq, Acquire)
//!     if s1 is odd: continue            # writer mid-update
//!     copy DATA into local buffer
//!     s2 = atomic_load(seq, Acquire)
//!     if s1 == s2: done                 # consistent snapshot
//! ```

use std::sync::atomic::{fence, AtomicU64, Ordering};

use crate::volume::Rgb;

pub const MAGIC: u32 = 0x315A_5650; // "PVZ1"
pub const VERSION: u32 = 1;
pub const DATA_OFFSET: usize = 64;

const OFF_MAGIC: usize = 0;
const OFF_VERSION: usize = 4;
const OFF_NUM_VOXELS: usize = 8;
const OFF_WIDTH: usize = 12;
const OFF_HEIGHT: usize = 16;
const OFF_LENGTH: usize = 20;
const OFF_CUBE_COUNT: usize = 24;
const OFF_SEQ: usize = 32;

/// Describes the published frame and knows how to write the header / publish a
/// frame into a raw byte region. Pure (no syscalls) so it is unit-testable
/// against an ordinary `Vec<u8>`.
#[derive(Debug, Clone, Copy)]
pub struct FrameFormat {
    pub num_voxels: usize,
    pub width: u32,
    pub height: u32,
    pub length: u32,
    pub cube_count: u32,
}

impl FrameFormat {
    pub fn total_size(&self) -> usize {
        DATA_OFFSET + self.num_voxels * 3
    }

    /// Write the static header fields and initialize `seq` to 0 (stable, empty).
    pub fn init_header(&self, buf: &mut [u8]) {
        assert!(buf.len() >= self.total_size(), "buffer too small for frame");
        put_u32(buf, OFF_MAGIC, MAGIC);
        put_u32(buf, OFF_VERSION, VERSION);
        put_u32(buf, OFF_NUM_VOXELS, self.num_voxels as u32);
        put_u32(buf, OFF_WIDTH, self.width);
        put_u32(buf, OFF_HEIGHT, self.height);
        put_u32(buf, OFF_LENGTH, self.length);
        put_u32(buf, OFF_CUBE_COUNT, self.cube_count);
        seq_atomic(buf).store(0, Ordering::Release);
    }

    /// Publish one frame using the seqlock protocol. `frame` must have exactly
    /// `num_voxels` entries.
    pub fn publish_into(&self, buf: &mut [u8], frame: &[Rgb]) {
        assert_eq!(frame.len(), self.num_voxels, "frame voxel count mismatch");

        // Derive the atomic through a raw pointer so its lifetime is *not* tied
        // to `buf`'s borrow — the seq field (offset 32) and the DATA region
        // (offset 64+) are disjoint, so writing DATA below while holding `seq`
        // is sound but the borrow checker can't see the disjointness.
        let seq = seq_atomic(buf);
        let start = seq.load(Ordering::Relaxed);
        // Mark "writer in progress" (odd) before touching data.
        seq.store(start.wrapping_add(1), Ordering::Release);
        fence(Ordering::Release);

        // SAFETY: frame is &[[u8;3]], contiguous; copy its bytes into DATA.
        let bytes = unsafe {
            std::slice::from_raw_parts(frame.as_ptr() as *const u8, frame.len() * 3)
        };
        buf[DATA_OFFSET..DATA_OFFSET + bytes.len()].copy_from_slice(bytes);

        // Mark "stable" (even); Release publishes the data writes above.
        seq.store(start.wrapping_add(2), Ordering::Release);
    }

    /// Reference reader (used by tests; the real reader is the UE component).
    /// Returns false if it couldn't get a consistent snapshot in `tries`.
    pub fn read_into(&self, buf: &[u8], out: &mut [Rgb], tries: usize) -> bool {
        let seq = seq_atomic(buf);
        for _ in 0..tries {
            let s1 = seq.load(Ordering::Acquire);
            if s1 & 1 != 0 {
                continue;
            }
            let bytes = unsafe {
                std::slice::from_raw_parts_mut(out.as_mut_ptr() as *mut u8, out.len() * 3)
            };
            bytes.copy_from_slice(&buf[DATA_OFFSET..DATA_OFFSET + bytes.len()]);
            let s2 = seq.load(Ordering::Acquire);
            if s1 == s2 {
                return true;
            }
        }
        false
    }
}

fn put_u32(buf: &mut [u8], off: usize, val: u32) {
    buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
}

/// Returns an `&AtomicU64` over the seq field. The returned reference's lifetime
/// is intentionally *decoupled* from `buf` (it's produced by dereferencing a raw
/// pointer), so callers can also write the disjoint DATA region while holding it.
///
/// SAFETY: OFF_SEQ (32) is 8-byte aligned within a page-aligned region, and the
/// region is always at least `total_size()` bytes. Atomic access through a shared
/// reference is the whole point of a seqlock.
fn seq_atomic<'a>(buf: &[u8]) -> &'a AtomicU64 {
    unsafe { &*(buf.as_ptr().add(OFF_SEQ) as *const AtomicU64) }
}

// --- mmap-backed writer -------------------------------------------------------

/// Owns a POSIX shared-memory region and publishes frames into it.
pub struct ShmemWriter {
    name: std::ffi::CString,
    ptr: *mut u8,
    size: usize,
    format: FrameFormat,
}

impl ShmemWriter {
    /// Create (or truncate) a shared-memory object named `name` (a leading '/'
    /// is added if absent, per POSIX) sized for `format`.
    pub fn create(name: &str, format: FrameFormat) -> Result<ShmemWriter, String> {
        let normalized = if name.starts_with('/') {
            name.to_string()
        } else {
            format!("/{name}")
        };
        let cname = std::ffi::CString::new(normalized.clone())
            .map_err(|_| "shm name contains NUL".to_string())?;
        let size = format.total_size();

        unsafe {
            // Remove any stale object from a previous run. On macOS a shm object
            // can only be ftruncate'd once (right after creation); reusing a
            // leftover one makes ftruncate fail with EINVAL, so always start
            // fresh. Errors (e.g. it doesn't exist) are intentionally ignored.
            libc::shm_unlink(cname.as_ptr());

            let fd = libc::shm_open(
                cname.as_ptr(),
                libc::O_CREAT | libc::O_RDWR,
                0o666 as libc::c_uint,
            );
            if fd < 0 {
                return Err(format!("shm_open({normalized}): {}", last_err()));
            }
            if libc::ftruncate(fd, size as libc::off_t) != 0 {
                let e = last_err();
                libc::close(fd);
                libc::shm_unlink(cname.as_ptr());
                return Err(format!("ftruncate({size}): {e}"));
            }
            let ptr = libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            );
            // The fd can be closed immediately; the mapping keeps the object alive.
            libc::close(fd);
            if ptr == libc::MAP_FAILED {
                libc::shm_unlink(cname.as_ptr());
                return Err(format!("mmap: {}", last_err()));
            }

            let writer = ShmemWriter { name: cname, ptr: ptr as *mut u8, size, format };
            format.init_header(writer.as_slice_mut());
            Ok(writer)
        }
    }

    fn as_slice_mut(&self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.size) }
    }

    pub fn publish(&self, frame: &[Rgb]) {
        self.format.publish_into(self.as_slice_mut(), frame);
    }

    pub fn format(&self) -> &FrameFormat {
        &self.format
    }
}

// SAFETY: the mapped region is process-shared memory at a stable address for the
// writer's lifetime. `publish` is a seqlock write that must be performed by one
// writer at a time — the receiver serializes all publishes under the volume mutex
// (see main.rs). Sharing the handle across listener threads is therefore sound.
unsafe impl Send for ShmemWriter {}
unsafe impl Sync for ShmemWriter {}

impl Drop for ShmemWriter {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr as *mut libc::c_void, self.size);
            libc::shm_unlink(self.name.as_ptr());
        }
    }
}

fn last_err() -> String {
    std::io::Error::last_os_error().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(n: usize) -> FrameFormat {
        FrameFormat { num_voxels: n, width: 20, height: 20, length: (n / 400) as u32, cube_count: 1 }
    }

    #[test]
    fn header_then_publish_roundtrips_via_reader() {
        let f = fmt(800);
        let mut region = vec![0u8; f.total_size()];
        f.init_header(&mut region);

        // Header sanity.
        assert_eq!(u32::from_le_bytes(region[0..4].try_into().unwrap()), MAGIC);
        assert_eq!(u32::from_le_bytes(region[8..12].try_into().unwrap()), 800);

        let mut frame = vec![[0u8; 3]; 800];
        for (i, px) in frame.iter_mut().enumerate() {
            *px = [(i & 0xff) as u8, (i >> 8) as u8, 0x42];
        }
        f.publish_into(&mut region, &frame);

        // seq is even (stable) after publish.
        let seq = u64::from_le_bytes(region[OFF_SEQ..OFF_SEQ + 8].try_into().unwrap());
        assert_eq!(seq % 2, 0);
        assert_eq!(seq, 2);

        let mut out = vec![[0u8; 3]; 800];
        assert!(f.read_into(&region, &mut out, 4));
        assert_eq!(out, frame);
    }

    #[test]
    fn seq_advances_by_two_each_publish() {
        let f = fmt(400);
        let mut region = vec![0u8; f.total_size()];
        f.init_header(&mut region);
        let frame = vec![[1u8, 2, 3]; 400];
        for expected in [2u64, 4, 6] {
            f.publish_into(&mut region, &frame);
            let seq = u64::from_le_bytes(region[OFF_SEQ..OFF_SEQ + 8].try_into().unwrap());
            assert_eq!(seq, expected);
        }
    }
}
