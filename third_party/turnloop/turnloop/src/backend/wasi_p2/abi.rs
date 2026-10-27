//! Allocation-free lowering for the variable-length p2 imports. Layouts match
//! wasip2 1.0.3's generated bindings (WASI 0.2.9, wasm32 canonical ABI).
//!
//! Only these synchronous imports install a scratch arena. They cannot call guest
//! user code. All returned lists are consumed before the arena is reused; none is
//! turned into a Vec or passed to a deallocator. Other imports use the ordinary
//! Rust allocator. std's cabi_realloc is intentionally weak for this purpose.
use crate::{Error, ErrorKind, Result};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6};
use std::{
    alloc::{Layout, alloc, handle_alloc_error, realloc},
    cell::Cell,
    ptr,
};
use wasip2::io::streams::{InputStream, StreamError};
use wasip2::sockets::udp::{IncomingDatagramStream, OutgoingDatagramStream};

#[derive(Clone, Copy)]
struct Arena {
    ptr: *mut u8,
    len: usize,
    used: usize,
    /// Largest alignment the storage base satisfies (4 for u32 words, 1 for bytes).
    align: usize,
}
thread_local! { static ARENA: Cell<Option<Arena>> = const { Cell::new(None) }; }
struct Reset;
impl Drop for Reset {
    fn drop(&mut self) {
        ARENA.set(None);
    }
}
fn scoped<T>(storage: &mut [u32], f: impl FnOnce() -> T) -> T {
    ARENA.with(|a| {
        assert!(a.get().is_none(), "nested canonical return arena");
        a.set(Some(Arena {
            ptr: storage.as_mut_ptr().cast(),
            len: storage.len() * 4,
            used: 0,
            align: 4,
        }));
    });
    let _reset = Reset;
    f()
}
/// Byte arena over caller output: a single lowered `list<u8>` lands in place.
fn scoped_bytes<T>(storage: &mut [u8], f: impl FnOnce() -> T) -> T {
    ARENA.with(|a| {
        assert!(a.get().is_none(), "nested canonical return arena");
        a.set(Some(Arena {
            ptr: storage.as_mut_ptr(),
            len: storage.len(),
            used: 0,
            align: 1,
        }));
    });
    let _reset = Reset;
    f()
}

// SAFETY: this replaces the weak std canonical realloc with the same C ABI.
// Active arenas exist only across our synchronous, non-reentrant imports. Outside
// that scope the canonical ABI allocator contract is forwarded to Rust unchanged.
#[unsafe(no_mangle)]
unsafe extern "C" fn cabi_realloc(
    old: *mut u8,
    old_len: usize,
    align: usize,
    len: usize,
) -> *mut u8 {
    assert!(align.is_power_of_two());
    if len == 0 {
        return ptr::without_provenance_mut(align);
    }
    if let Some(mut arena) = ARENA.get() {
        // These imports lower lists once, not incrementally via realloc.
        assert_eq!(old_len, 0);
        assert!(align <= arena.align);
        let offset = (arena.used + align - 1) & !(align - 1);
        assert!(
            len <= arena.len.saturating_sub(offset),
            "canonical result exceeds reserved capacity"
        );
        arena.used = offset + len;
        ARENA.set(Some(arena));
        // SAFETY: the checked allocation lies inside the live exclusive arena.
        return unsafe { arena.ptr.add(offset) };
    }
    let layout = Layout::from_size_align(if old_len == 0 { len } else { old_len }, align)
        .expect("canonical layout");
    // SAFETY: outside an arena, pointers come from this allocator; the canonical
    // caller supplies their original length/alignment and a valid new size.
    let result = unsafe {
        if old_len == 0 {
            alloc(layout)
        } else {
            realloc(old, layout, len)
        }
    };
    if result.is_null() {
        handle_alloc_error(layout);
    }
    result
}

#[link(wasm_import_module = "wasi:io/poll@0.2.9")]
unsafe extern "C" {
    #[link_name = "poll"]
    fn raw_poll(input: *const u32, len: usize, result: *mut usize);
}
#[link(wasm_import_module = "wasi:io/streams@0.2.9")]
unsafe extern "C" {
    #[link_name = "[method]input-stream.read"]
    fn raw_read(handle: u32, len: u64, result: *mut u32);
}
#[link(wasm_import_module = "wasi:sockets/udp@0.2.9")]
unsafe extern "C" {
    #[link_name = "[method]incoming-datagram-stream.receive"]
    fn raw_receive(handle: u32, len: u64, result: *mut u32);
    #[link_name = "[method]outgoing-datagram-stream.send"]
    fn raw_send(handle: u32, input: *const u32, len: usize, result: *mut u64);
}

#[link(wasm_import_module = "wasi:filesystem/types@0.2.9")]
unsafe extern "C" {
    #[link_name = "[method]descriptor.read"]
    fn raw_file_read(handle: i32, length: i64, offset: i64, result: *mut u8);
}
/// `descriptor.read` lowered directly into `output`. Returns the byte count and
/// the end-of-file flag, or the raw filesystem error-code discriminant.
pub fn file_read(
    descriptor: &wasip2::filesystem::types::Descriptor,
    output: &mut [u8],
    offset: u64,
) -> std::result::Result<(usize, bool), u8> {
    let mut result = [0u32; 4];
    let base = output.as_mut_ptr();
    let capacity = output.len();
    scoped_bytes(output, || {
        // SAFETY: live descriptor, bounded length, aligned 16-byte result area. The
        // only list the host lowers is the returned bytes, placed in the arena.
        unsafe {
            raw_file_read(
                descriptor.handle() as i32,
                capacity as i64,
                offset as i64,
                result.as_mut_ptr().cast(),
            );
        }
        match result[0] & 0xff {
            0 => {
                let (pointer, n) = (result[1] as usize as *mut u8, result[2] as usize);
                assert!(n <= capacity);
                if n != 0 && pointer != base {
                    // SAFETY: host-initialized bytes inside the output arena; ptr::copy
                    // tolerates overlap with the destination prefix.
                    unsafe { ptr::copy(pointer, base, n) };
                }
                Ok((n, result[3] & 0xff != 0))
            }
            1 => Err((result[1] & 0xff) as u8),
            _ => unreachable!("invalid read result"),
        }
    })
}

pub fn poll(input: &[u32], storage: &mut [u32], ready: &mut Vec<usize>) {
    assert!(!input.is_empty());
    let mut result = [0usize; 2];
    scoped(storage, || {
        // SAFETY: handles are borrowed live Pollables, input is initialized and
        // the aligned two-word return area is writable. Arena covers N indices.
        unsafe {
            raw_poll(input.as_ptr(), input.len(), result.as_mut_ptr());
        }
        assert!(result[1] <= input.len());
        if result[1] == 0 {
            return;
        }
        // SAFETY: canonical lowering initialized precisely result[1] u32 indices.
        let indices = unsafe { std::slice::from_raw_parts(result[0] as *const u32, result[1]) };
        for &i in indices {
            assert!((i as usize) < input.len());
            ready.push(i as usize);
        }
    });
}
pub fn read(
    stream: &InputStream,
    output: &mut [u8],
    storage: &mut [u32],
) -> std::result::Result<usize, StreamError> {
    let mut result = [0u32; 3];
    scoped(storage, || {
        // SAFETY: live input resource, bounded requested length, aligned result.
        unsafe {
            raw_read(stream.handle(), output.len() as u64, result.as_mut_ptr());
        }
        match result[0] & 0xff {
            0 => {
                let n = result[2] as usize;
                assert!(n <= output.len());
                if n == 0 {
                    return Ok(0);
                }
                // SAFETY: host initialized n bytes in the active arena; output is exclusive.
                unsafe {
                    ptr::copy_nonoverlapping(result[1] as *const u8, output.as_mut_ptr(), n);
                }
                Ok(n)
            }
            1 if result[1] & 0xff == 1 => Err(StreamError::Closed),
            1 => {
                // SAFETY: canonical result transfers this owned error resource.
                Err(StreamError::LastOperationFailed(unsafe {
                    wasip2::io::error::Error::from_handle(result[2])
                }))
            }
            _ => unreachable!("invalid stream result"),
        }
    })
}
fn decode_addr(bytes: &[u8]) -> SocketAddr {
    let port = u16::from_le_bytes([bytes[4], bytes[5]]);
    if bytes[0] == 0 {
        SocketAddr::from((Ipv4Addr::new(bytes[6], bytes[7], bytes[8], bytes[9]), port))
    } else {
        let segments =
            std::array::from_fn(|i| u16::from_le_bytes([bytes[12 + i * 2], bytes[13 + i * 2]]));
        let flow = u32::from_le_bytes(bytes[8..12].try_into().expect("flow"));
        let scope = u32::from_le_bytes(bytes[28..32].try_into().expect("scope"));
        SocketAddr::V6(SocketAddrV6::new(
            Ipv6Addr::from(segments),
            port,
            flow,
            scope,
        ))
    }
}
pub fn receive(
    stream: &IncomingDatagramStream,
    output: &mut [u8],
    storage: &mut [u32],
) -> Result<Option<(usize, SocketAddr)>> {
    let mut result = [0u32; 3];
    scoped(storage, || {
        // SAFETY: request at most one datagram; arena reserves the maximum UDP
        // payload plus its 40-byte canonical record. All storage remains live.
        unsafe {
            raw_receive(stream.handle(), 1, result.as_mut_ptr());
        }
        if result[0] & 0xff != 0 {
            return Err(Error::new(ErrorKind::Other));
        }
        if result[2] == 0 {
            return Ok(None);
        }
        assert_eq!(result[2], 1);
        // SAFETY: result points to one initialized 40-byte canonical datagram.
        let record = unsafe { std::slice::from_raw_parts(result[1] as *const u32, 10) };
        let n = (record[1] as usize).min(output.len());
        // SAFETY: canonical payload is live and contains record[1] initialized bytes.
        if n != 0 {
            // SAFETY: nonempty canonical payload and exclusive output are live.
            unsafe {
                ptr::copy_nonoverlapping(record[0] as *const u8, output.as_mut_ptr(), n);
            }
        }
        // SAFETY: last 32 bytes contain the canonical IP address variant.
        let address = unsafe { std::slice::from_raw_parts(record.as_ptr().add(2).cast(), 32) };
        Ok(Some((n, decode_addr(address))))
    })
}
pub fn send(stream: &OutgoingDatagramStream, bytes: &[u8], to: SocketAddr) -> Result<usize> {
    if bytes.len() > 65535 {
        return Err(Error::new(ErrorKind::InvalidInput));
    }
    let mut record = [0u32; 11];
    record[0] = bytes.as_ptr() as u32;
    record[1] = bytes.len() as u32;
    record[2] = 1;
    // SAFETY: the initialized record is exclusively borrowed as its byte representation.
    let address =
        unsafe { std::slice::from_raw_parts_mut(record.as_mut_ptr().add(3).cast::<u8>(), 32) };
    address[4..6].copy_from_slice(&to.port().to_le_bytes());
    match to {
        SocketAddr::V4(a) => address[6..10].copy_from_slice(&a.ip().octets()),
        SocketAddr::V6(a) => {
            address[0] = 1;
            address[8..12].copy_from_slice(&a.flowinfo().to_le_bytes());
            for (i, s) in a.ip().segments().into_iter().enumerate() {
                address[12 + i * 2..14 + i * 2].copy_from_slice(&s.to_le_bytes());
            }
            address[28..32].copy_from_slice(&a.scope_id().to_le_bytes());
        }
    }
    let mut result = [0u64; 2];
    // SAFETY: single initialized 44-byte canonical datagram borrows stable bytes;
    // result is a 16-byte, 8-aligned result<u64,error-code> return area.
    unsafe {
        raw_send(stream.handle(), record.as_ptr(), 1, result.as_mut_ptr());
    }
    if result[0] & 0xff != 0 {
        return Err(Error::new(ErrorKind::Other));
    }
    if result[1] == 0 {
        return Err(Error::new(ErrorKind::WouldBlock));
    }
    assert_eq!(result[1], 1);
    Ok(bytes.len())
}
