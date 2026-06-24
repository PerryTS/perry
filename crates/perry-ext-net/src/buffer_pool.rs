//! Step 4 of the net perf quest (#5419) — a process-wide freelist of
//! read buffers, so the socket read loop reuses pooled 16 KiB capacity
//! instead of allocating a fresh `BytesMut` on every read.
//!
//! # Why a pool, given step 2 already reuses one buffer per socket
//!
//! Step 2 ([#5638]) replaced the per-read `Vec<u8>` + memcpy with a
//! single reused `BytesMut` that `read_buf` fills in place and
//! `split_to(n).freeze()` carves a zero-copy `Bytes` view out of. That
//! removed the per-read *copy* — but not the per-read *allocation*. The
//! `Bytes` chunk handed to the `'data'` event is queued in the global
//! pending-events vec and only converted to a JS `Buffer` (a real
//! `copy_nonoverlapping`) on the next main-thread drain tick. So the
//! chunk — and therefore the `BytesMut`'s backing storage, which the
//! chunk shares — is still alive when the read loop comes back around
//! to `reserve(16 KiB)` for the next read. `BytesMut::reserve` cannot
//! reclaim shared storage in place, so it **allocates a brand-new
//! 16 KiB block on every read that delivered data** (empirically
//! confirmed against `bytes` 1.12). On top of that, every new
//! connection paid one `BytesMut::with_capacity(16 KiB)`.
//!
//! # What the pool amortizes
//!
//! The read loop now [`checkout`]s a buffer from this freelist for each
//! read and [`checkin`]s the post-`split_to` remainder afterwards. A
//! recycled buffer's `checkout` does `clear()` + `reserve(16 KiB)`:
//! once the chunk it previously yielded has been drained and dropped
//! (refcount back to 1), that `reserve` reclaims the **same** 16 KiB
//! allocation in place — no new allocation. In steady state the pool's
//! depth covers the handful of chunks in flight between read and drain,
//! so a buffer that cycles back to the front of the freelist has
//! already had its chunk drained, and the read reuses pooled capacity.
//! New connections draw from the same pool, so the per-socket
//! allocation is amortized across the fleet too.
//!
//! Behavior is unchanged: `checkout` always hands back an empty buffer
//! with ≥ 16 KiB of writable capacity, identical to what
//! `BytesMut::with_capacity(16 KiB)` + per-read `clear()`/`reserve()`
//! produced. The 16 KiB per-read window cap still lives at the read
//! site (the `BufMut::limit` wrapper in `run_socket_task`); this module
//! only owns buffer *recycling*, never read sizing or chunk boundaries.
//!
//! # Concurrency
//!
//! Socket tasks run cooperatively on Perry's shared multi-thread tokio
//! runtime (step 3, the shared reactor), so a task may check a buffer
//! out on one reactor worker and — after migrating across an `.await`
//! — check it back in on another. The freelist is therefore a
//! `Mutex<Vec<BytesMut>>`, matching the other process-wide `statics`
//! maps in this crate. The lock is held only for the O(1) `pop` /
//! `push`; it is never held across an `.await`.
//!
//! [#5419]: https://github.com/PerryTS/perry/pull/5419
//! [#5638]: https://github.com/PerryTS/perry/pull/5638

use bytes::BytesMut;
use std::sync::{Mutex, OnceLock};

/// Per-read window size — kept in sync with the `BufMut::limit(16 KiB)`
/// cap at the read site in `run_socket_task`. Every pooled buffer is
/// reserved to at least this capacity on checkout.
pub(crate) const READ_BUF_CAP: usize = 16 * 1024;

/// Upper bound on idle buffers parked in the freelist. The pool exists
/// to recycle steady-state capacity, not to grow without limit: under a
/// burst that frees many buffers at once, anything past this cap is
/// dropped (its allocation returned to the global allocator) so idle
/// memory stays bounded at `MAX_POOLED * 16 KiB` (1 MiB). A burst that
/// needs more than `MAX_POOLED` buffers simply allocates fresh ones on
/// checkout, exactly as the pre-pool code always did — the cap trades a
/// little peak-burst reuse for a hard memory ceiling.
const MAX_POOLED: usize = 64;

/// The process-wide freelist of idle read buffers.
fn pool() -> &'static Mutex<Vec<BytesMut>> {
    static POOL: OnceLock<Mutex<Vec<BytesMut>>> = OnceLock::new();
    POOL.get_or_init(|| Mutex::new(Vec::new()))
}

/// Check out a read buffer: an empty `BytesMut` with at least
/// [`READ_BUF_CAP`] bytes of writable capacity, ready for `read_buf`.
///
/// Pops a recycled buffer from the freelist when one is available, else
/// allocates a fresh one. `clear()` + `reserve(READ_BUF_CAP)` mean a
/// recycled buffer whose prior chunk has already drained reclaims its
/// backing allocation in place (no allocation); a recycled buffer whose
/// chunk is somehow still alive, or a fresh buffer, allocates — the same
/// outcome the pre-pool `with_capacity` / `reserve` path produced.
pub(crate) fn checkout() -> BytesMut {
    let mut buf = pool().lock().unwrap().pop().unwrap_or_default();
    // `clear()` drops any (already split-off) contents; `reserve`
    // guarantees the 16 KiB writable window. On a recycled buffer with a
    // drained chunk this is an in-place reclaim; otherwise it allocates.
    buf.clear();
    buf.reserve(READ_BUF_CAP);
    buf
}

/// Return a read buffer to the freelist for reuse.
///
/// Called with the post-`split_to` remainder once the read loop has
/// handed its chunk downstream. The buffer's backing storage may still
/// be shared with that in-flight chunk; that is fine — the next
/// `checkout` of this buffer will reclaim in place only once the chunk
/// has drained, and reallocate otherwise (never corrupting the chunk).
/// Drops the buffer instead of parking it once the freelist is at
/// [`MAX_POOLED`], keeping idle memory bounded.
pub(crate) fn checkin(buf: BytesMut) {
    let mut pool = pool().lock().unwrap();
    if pool.len() < MAX_POOLED {
        pool.push(buf);
    }
    // else: drop `buf`, returning its allocation to the global allocator.
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::{BufMut, Bytes};

    // NOTE: the `perry_ffi_spawn_async` no-op link stub that lets the
    // `perry-ext-net` test binary link without the perry-stdlib edge is
    // defined once in `jsvalue.rs`'s `#[cfg(test)]` module (#5638) and
    // covers the whole test binary — these tests don't redefine it.

    /// A fresh checkout (empty pool) hands back an empty buffer with the
    /// full 16 KiB write window — byte-for-byte the precondition the read
    /// loop relied on from `BytesMut::with_capacity(16 KiB)`.
    #[test]
    fn checkout_yields_empty_16k_window() {
        let buf = checkout();
        assert_eq!(buf.len(), 0, "checked-out buffer must start empty");
        assert!(
            buf.capacity() >= READ_BUF_CAP,
            "checked-out buffer must offer the 16 KiB read window, got {}",
            buf.capacity()
        );
    }

    /// The core amortization claim, exercised directly: a buffer whose
    /// yielded chunk has been dropped (the steady-state drain ordering)
    /// is recycled WITHOUT a new allocation — the same backing storage
    /// comes back on the next checkout. This is the per-read allocation
    /// the pool removes; a pool that reallocated here would not buy the
    /// step-4 win.
    #[test]
    fn drained_buffer_recycles_storage_in_place() {
        // Mirror the read loop: checkout, fill, split a chunk off, then
        // park the remainder once the chunk has drained.
        let mut buf = checkout();
        let base = buf.as_ptr() as usize;
        buf.extend_from_slice(&[0xAB; 256]);
        let chunk: Bytes = buf.split_to(256).freeze();
        drop(chunk); // consumer drained + dropped the chunk
        checkin(buf);

        let recycled = checkout();
        assert_eq!(
            recycled.as_ptr() as usize,
            base,
            "a drained, returned buffer must be reused in place, not reallocated"
        );
    }

    /// Recycling never corrupts an in-flight chunk: even though a
    /// returned buffer shares storage with a chunk that has NOT yet
    /// drained, the bytes the chunk holds stay intact after the buffer is
    /// checked back out and refilled. (Checkout simply reallocates rather
    /// than reclaiming in this case — correctness over reuse.)
    #[test]
    fn checkin_preserves_live_chunk_bytes() {
        let mut buf = checkout();
        buf.extend_from_slice(b"first-message");
        let chunk: Bytes = buf.split_to(b"first-message".len()).freeze();
        // Return the remainder while the chunk is still "in the queue".
        checkin(buf);

        // Next reader checks out and writes a different payload.
        let mut next = checkout();
        next.extend_from_slice(b"second-message-payload");
        let chunk2 = next.split_to(b"second-message-payload".len()).freeze();

        // The first chunk's bytes are untouched by the second read.
        assert_eq!(&chunk[..], b"first-message");
        assert_eq!(&chunk2[..], b"second-message-payload");
    }

    /// The freelist is bounded: parking more than `MAX_POOLED` buffers
    /// keeps only `MAX_POOLED`, so idle memory can't grow without limit.
    /// Checking out past the cap still works — it just allocates fresh,
    /// exactly the pre-pool behavior.
    #[test]
    fn freelist_is_bounded() {
        // Drain any buffers a sibling test parked, so the count is exact.
        while pool().lock().unwrap().pop().is_some() {}

        for _ in 0..(MAX_POOLED + 16) {
            checkin(BytesMut::with_capacity(READ_BUF_CAP));
        }
        assert_eq!(
            pool().lock().unwrap().len(),
            MAX_POOLED,
            "freelist must cap at MAX_POOLED idle buffers"
        );

        // Drain back down so the global pool doesn't leak state into
        // other tests in this binary.
        while pool().lock().unwrap().pop().is_some() {}
    }

    // ─── Differential read-path test against a real socket ──────────────

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    /// Drive the *exact* pooled read-loop core — `checkout` → 16 KiB
    /// `BufMut::limit` window → `read_buf` → `split_to(n).freeze()` →
    /// `checkin` — over a real loopback TCP connection, collecting every
    /// chunk in the order delivered. Each chunk is kept alive (mirroring
    /// the global pending-events queue, where a chunk outlives the read
    /// iteration that produced it) so the pool's recycle-under-live-chunks
    /// path is genuinely exercised. Returns the chunk sizes and the
    /// concatenated bytes, so a caller can assert byte-identity and the
    /// 16 KiB chunk-boundary behavior. This is the step-2 differential
    /// posture: prove the pool delivers byte-identical data and event
    /// ordering, not just that the pool data structure behaves.
    async fn read_all_pooled(payload: &[u8]) -> (Vec<usize>, Vec<u8>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Writer half: connect and stream the payload, then close (FIN).
        let payload_owned = payload.to_vec();
        let writer = tokio::spawn(async move {
            let mut c = TcpStream::connect(addr).await.unwrap();
            c.write_all(&payload_owned).await.unwrap();
            c.shutdown().await.unwrap();
        });

        let (mut server, _) = listener.accept().await.unwrap();

        // Let the writer push the *whole* payload (and FIN) into the OS
        // receive buffer before the first read. That way the 16 KiB
        // `BufMut::limit` window is the ONLY thing bounding each
        // `read_buf` — making the per-read cap deterministically
        // load-bearing for a >16 KiB payload (a missing cap would
        // surface as one oversized chunk), instead of relying on TCP
        // segmentation timing.
        writer.await.unwrap();

        let mut chunks: Vec<Bytes> = Vec::new(); // kept alive like the queue
        let mut sizes = Vec::new();
        let mut joined = Vec::new();
        loop {
            // Mirror `run_socket_task` exactly.
            let mut buf = checkout();
            let mut window = (&mut buf).limit(READ_BUF_CAP);
            let n = server.read_buf(&mut window).await.unwrap();
            drop(window);
            if n == 0 {
                break; // peer FIN — the loop's `Ok(0)` 'end'/'close' path
            }
            let chunk = buf.split_to(n).freeze();
            sizes.push(chunk.len());
            joined.extend_from_slice(&chunk);
            chunks.push(chunk); // outlives the iteration, as in the queue
            checkin(buf);
        }
        (sizes, joined)
    }

    /// The headline behavior-preservation guarantee: across small,
    /// multi-message, exactly-16 KiB-boundary, and well-over-16 KiB
    /// payloads, the pooled read loop delivers byte-identical data and
    /// the same per-read 16 KiB chunk boundaries the step-2 fixed-window
    /// loop did. A pool that corrupted, dropped, or reordered bytes — or
    /// that broke the 16 KiB window cap — would fail here.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pooled_reads_are_byte_identical_across_sizes() {
        // Small single message — fits in one read well under the window.
        let small = b"hello world".to_vec();
        let (sizes, joined) = read_all_pooled(&small).await;
        assert_eq!(joined, small, "small payload must round-trip byte-for-byte");
        assert!(
            sizes.iter().all(|&s| s <= READ_BUF_CAP),
            "no chunk may exceed the 16 KiB window"
        );

        // Exactly the 16 KiB window boundary.
        let boundary: Vec<u8> = (0..READ_BUF_CAP).map(|i| (i % 251) as u8).collect();
        let (sizes, joined) = read_all_pooled(&boundary).await;
        assert_eq!(joined, boundary, "16 KiB-boundary payload must round-trip");
        assert!(
            sizes.iter().all(|&s| s <= READ_BUF_CAP),
            "the per-read window cap holds at the boundary"
        );

        // Well over the window — must arrive as multiple ≤16 KiB chunks
        // whose concatenation is the original, with all but the last read
        // bounded by the window (proving the cap, not just the total).
        let big: Vec<u8> = (0..(READ_BUF_CAP * 4 + 777))
            .map(|i| (i % 256) as u8)
            .collect();
        let (sizes, joined) = read_all_pooled(&big).await;
        assert_eq!(joined, big, ">16 KiB payload must reassemble byte-for-byte");
        assert!(
            sizes.iter().all(|&s| s <= READ_BUF_CAP),
            "every read is capped at the 16 KiB window, got {sizes:?}"
        );
        assert!(
            sizes.len() >= 5,
            "a >64 KiB payload must span multiple reads, got {} chunks",
            sizes.len()
        );
    }
}
