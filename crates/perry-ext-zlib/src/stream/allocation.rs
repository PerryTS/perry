//! Codec working memory. Brotli's state and every brotli allocation, and the
//! inflate state (window and tables), live in the payload's `PayloadBuffer`s,
//! counted exactly by its `BufferOwner`; brotli reaches them through the
//! owner's C allocator hook. zstd's contexts report `sizeof`. The deflate
//! compressor boxes its arrays inside miniz_oxide, so its fixed workspace is
//! accounted from the pinned backend's actual boxed sizes.
//!
//! Out of memory: the hook returns null, and [`BufferAlloc`] hands brotli an
//! empty block, which is brotli's own allocation-failure signal. The decoder
//! checks it and fails the stream with `BROTLI_DECODER_ERROR_ALLOC_*` (the
//! stream emits `error` with that code, then `close`; see
//! `brotli_decoder_reports_a_refused_allocation`). The encoder does not check
//! and indexes the empty block, which panics, i.e. aborts under perry's
//! panic=abort, the same outcome as its stock allocator's out-of-memory abort.
//! State structs placed by [`Placed`] report `ErrorKind::OutOfMemory`.
use brotli::enc::{
    cluster::HistogramPair,
    command::Command,
    entropy_encode::HuffmanTree,
    floatX,
    histogram::{ContextType, HistogramCommand, HistogramDistance, HistogramLiteral},
    s16, v8, StaticCommand, ZopfliNode, PDF,
};
use brotli::{Allocator, HuffmanCode, SliceWrapper, SliceWrapperMut};
use perry_ffi::native_payload::buffer::{BufferOwner, CAllocHook, PayloadBuffer, ALIGN};
use std::marker::PhantomData;
use std::ptr::NonNull;

/// How a zeroed hook block becomes `len` defaults of `T`.
///
/// # Safety
/// `ZERO_IS_DEFAULT = true` promises that all-zero bytes are a valid `T`
/// equal to `T::default()`: the block is then used as is, and its untouched
/// pages stay uncommitted (as std's `vec![0; n]` does). Otherwise every
/// element is written with `T::default()`.
pub(super) unsafe trait HookFill: Default + Clone {
    const ZERO_IS_DEFAULT: bool;
}
macro_rules! hook_fill {
    ($zero:literal: $($t:ty),*) => {
        $(unsafe impl HookFill for $t { const ZERO_IS_DEFAULT: bool = $zero; })*
    };
}
// Primitive integers and floats: zero is their default. (`floatX` is `f32`.)
hook_fill!(true: u8, u16, u32, u64, i32, floatX);
// Brotli's structs and vector types: written element by element.
hook_fill!(false: HuffmanCode, Command, v8, s16, PDF, StaticCommand, HistogramLiteral,
    HistogramCommand, HistogramDistance, HistogramPair, ContextType, HuffmanTree, ZopfliNode);

/// Brotli's allocator: its custom allocator hook, bound to the payload's
/// owner. Every clone and every block keeps the owner (and so the hook's
/// `opaque`) alive.
#[derive(Clone)]
pub(super) struct BufferAlloc {
    hook: CAllocHook,
    owner: BufferOwner,
}
impl BufferAlloc {
    pub(super) fn new(owner: &BufferOwner) -> Self {
        Self {
            hook: owner.hook(),
            owner: owner.clone(),
        }
    }
    /// A hook that refuses every request, as an exhausted backing does.
    #[cfg(test)]
    pub(super) fn refusing(owner: &BufferOwner) -> Self {
        unsafe extern "C" fn refuse(_: *mut std::ffi::c_void, _: usize) -> *mut std::ffi::c_void {
            std::ptr::null_mut()
        }
        let mut alloc = Self::new(owner);
        alloc.hook.alloc_func = refuse;
        alloc
    }
}

/// One brotli block: freed through the hook when brotli frees it, or on drop.
/// It holds its owner, so the ledger outlives every block by construction.
pub(super) struct HookBlock<T> {
    ptr: NonNull<T>,
    len: usize,
    held: Option<(CAllocHook, BufferOwner)>,
}
impl<T> Default for HookBlock<T> {
    fn default() -> Self {
        Self {
            ptr: NonNull::dangling(),
            len: 0,
            held: None,
        }
    }
}
impl<T> SliceWrapper<T> for HookBlock<T> {
    fn slice(&self) -> &[T] {
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }
}
impl<T> SliceWrapperMut<T> for HookBlock<T> {
    fn slice_mut(&mut self) -> &mut [T] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }
}
impl<T> Drop for HookBlock<T> {
    fn drop(&mut self) {
        if let Some((hook, _owner)) = &self.held {
            unsafe {
                std::ptr::drop_in_place(std::ptr::slice_from_raw_parts_mut(
                    self.ptr.as_ptr(),
                    self.len,
                ));
                (hook.free_func)(hook.opaque, self.ptr.as_ptr().cast());
            }
        }
    }
}

impl<T: HookFill> Allocator<T> for BufferAlloc {
    type AllocatedMemory = HookBlock<T>;
    fn alloc_cell(&mut self, len: usize) -> HookBlock<T> {
        assert!(std::mem::align_of::<T>() <= ALIGN);
        let size = std::mem::size_of::<T>();
        if len == 0 || size == 0 {
            return HookBlock {
                ptr: NonNull::dangling(),
                len: if size == 0 { len } else { 0 },
                held: None,
            };
        }
        let Some(bytes) = len.checked_mul(size) else {
            // brotli reads an empty block as its allocation failure.
            return HookBlock::default();
        };
        let ptr = unsafe { (self.hook.alloc_func)(self.hook.opaque, bytes) } as *mut T;
        let Some(ptr) = NonNull::new(ptr) else {
            return HookBlock::default();
        };
        if !T::ZERO_IS_DEFAULT {
            for i in 0..len {
                unsafe { ptr.as_ptr().add(i).write(T::default()) };
            }
        }
        HookBlock {
            ptr,
            len,
            held: Some((self.hook, self.owner.clone())),
        }
    }
    fn free_cell(&mut self, memory: HookBlock<T>) {
        drop(memory);
    }
}
impl brotli::enc::combined_alloc::BrotliAlloc for BufferAlloc {}

/// One codec state struct placed in a payload buffer instead of a `Box`.
pub(super) struct Placed<T> {
    buffer: PayloadBuffer,
    _type: PhantomData<T>,
}
impl<T> Placed<T> {
    /// Move `value` into a buffer; `None` when the backing refuses.
    pub(super) fn try_new(owner: &BufferOwner, value: T) -> Option<Self> {
        let mut buffer = Self::buffer(owner)?;
        unsafe { buffer.as_mut_ptr().cast::<T>().write(value) };
        Some(Self {
            buffer,
            _type: PhantomData,
        })
    }
    /// The buffer's zero bytes, read as a `T` in place (no stack copy).
    ///
    /// # Safety
    /// All-zero bytes are a valid `T`.
    pub(super) unsafe fn try_zeroed(owner: &BufferOwner) -> Option<Self> {
        Some(Self {
            buffer: Self::buffer(owner)?,
            _type: PhantomData,
        })
    }
    fn buffer(owner: &BufferOwner) -> Option<PayloadBuffer> {
        assert!(std::mem::align_of::<T>() <= ALIGN);
        PayloadBuffer::alloc(owner, std::mem::size_of::<T>())
    }
}
impl<T> std::ops::Deref for Placed<T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.buffer.as_ptr().cast::<T>() }
    }
}
impl<T> std::ops::DerefMut for Placed<T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.buffer.as_mut_ptr().cast::<T>() }
    }
}
impl<T> Drop for Placed<T> {
    fn drop(&mut self) {
        unsafe { std::ptr::drop_in_place(self.buffer.as_mut_ptr().cast::<T>()) };
    }
}

/// miniz_oxide's inflate state, built in its buffer. With the pinned
/// miniz_oxide 0.9.1 every field of `InflateState` accepts all-zero bytes
/// (integers, byte arrays, `bool`s, and enums whose zero discriminant exists:
/// `State::Start`, `TINFLStatus::Done`, `DataFormat::Zlib`), and
/// `reset(format)` then sets exactly what `InflateState::new(format)` sets.
pub(super) fn inflate_state(
    owner: &BufferOwner,
    format: miniz_oxide::DataFormat,
) -> std::io::Result<Placed<miniz_oxide::inflate::stream::InflateState>> {
    let mut state: Placed<miniz_oxide::inflate::stream::InflateState> =
        unsafe { Placed::try_zeroed(owner) }.ok_or_else(out_of_memory)?;
    state.reset(format);
    Ok(state)
}

pub(super) fn out_of_memory() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::OutOfMemory, "zlib codec state")
}

/// miniz_oxide 0.9.1 (pinned) boxes the deflate dictionary (32 KiB plus 258
/// lookahead bytes), its 32 Ki-entry `next` and `hash` u16 chains, and three
/// 288-symbol Huffman tables (u16 counts, u16 codes, u8 sizes) outside
/// `CompressorOxide` itself.
pub(super) fn deflate_bytes() -> usize {
    const DICT: usize = 32_768 + 258;
    const CHAINS: usize = 2 * 32_768 * std::mem::size_of::<u16>();
    const HUFFMAN: usize = 3 * 288 * (2 + 2 + 1);
    std::mem::size_of::<miniz_oxide::deflate::core::CompressorOxide>() + DICT + CHAINS + HUFFMAN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_place_inflate_state_matches_a_constructed_one() {
        let owner = BufferOwner::new();
        let input = crate::deflate_bytes(b"in place, not on the stack").unwrap();
        for format in [miniz_oxide::DataFormat::Zlib, miniz_oxide::DataFormat::Raw] {
            let mut placed = inflate_state(&owner, format).unwrap();
            let mut boxed = miniz_oxide::inflate::stream::InflateState::new_boxed(format);
            let (mut a, mut b) = ([0u8; 64], [0u8; 64]);
            let ra = miniz_oxide::inflate::stream::inflate(
                &mut placed,
                &input,
                &mut a,
                miniz_oxide::MZFlush::Finish,
            );
            let rb = miniz_oxide::inflate::stream::inflate(
                &mut boxed,
                &input,
                &mut b,
                miniz_oxide::MZFlush::Finish,
            );
            assert_eq!(ra.status, rb.status);
            assert_eq!(
                (ra.bytes_consumed, ra.bytes_written),
                (rb.bytes_consumed, rb.bytes_written)
            );
            assert_eq!(a, b);
            assert_eq!(placed.last_status(), boxed.last_status());
        }
        assert_eq!(owner.bytes(), 0);
    }

    #[test]
    fn ffi_buffer_grows_from_empty_and_in_small_steps() {
        let owner = BufferOwner::new();
        let mut buffer = PayloadBuffer::alloc(&owner, 0).unwrap();
        assert!(buffer.is_empty());
        assert_eq!(owner.bytes(), 0);
        assert!(buffer.grow(100));
        assert_eq!((buffer.len(), owner.bytes()), (100, 100));
        assert_eq!(buffer.as_ptr() as usize % ALIGN, 0);
        assert!(buffer.as_slice().iter().all(|b| *b == 0));
        buffer.as_mut_slice().fill(9);
        assert!(buffer.grow(104));
        assert_eq!((buffer.len(), owner.bytes()), (104, 104));
        assert!(buffer.as_slice()[..100].iter().all(|b| *b == 9));
        assert!(buffer.as_slice()[100..].iter().all(|b| *b == 0));
        assert!(!buffer.grow(usize::MAX), "a refused grow reports false");
        assert_eq!((buffer.len(), owner.bytes()), (104, 104));
        assert!(buffer.as_slice()[..100].iter().all(|b| *b == 9));
        buffer.shrink(8);
        assert_eq!((buffer.len(), owner.bytes()), (8, 8));
        drop(buffer);
        assert_eq!(owner.bytes(), 0);
    }

    #[test]
    fn blocks_keep_their_owner_and_refusals_are_empty_blocks() {
        let owner = BufferOwner::new();
        let mut alloc = BufferAlloc::new(&owner);
        let block: HookBlock<u32> = alloc.alloc_cell(1000);
        let probe = owner.clone();
        drop(owner);
        drop(alloc);
        assert!(probe.bytes() >= 4000, "the block still counts");
        drop(block);
        assert_eq!(probe.bytes(), 0);
        let mut alloc = BufferAlloc::new(&probe);
        let overflow: HookBlock<u64> = alloc.alloc_cell(usize::MAX / 2);
        assert!(overflow.slice().is_empty());
        let refused: HookBlock<u8> = BufferAlloc::refusing(&probe).alloc_cell(64);
        assert!(refused.slice().is_empty());
        assert_eq!(probe.bytes(), 0);
    }

    /// The decoder turns a refused hook allocation into its ALLOC error code.
    #[test]
    fn brotli_decoder_reports_a_refused_allocation() {
        let owner = BufferOwner::new();
        let alloc = BufferAlloc::refusing(&owner);
        let mut state = brotli::BrotliState::new(alloc.clone(), alloc.clone(), alloc);
        let input = super::super::brotli_compress_bytes(&vec![7u8; 100_000]);
        let mut out = vec![0u8; 4096];
        let (mut avail_in, mut in_off, mut avail_out, mut out_off, mut total) =
            (input.len(), 0, out.len(), 0, 0);
        let result = brotli::BrotliDecompressStream(
            &mut avail_in,
            &mut in_off,
            &input,
            &mut avail_out,
            &mut out_off,
            &mut out,
            &mut total,
            &mut state,
        );
        assert!(matches!(result, brotli::BrotliResult::ResultFailure));
        let code = format!("{:?}", state.error_code);
        assert!(code.contains("ALLOC"), "{code}");
        assert_eq!(owner.bytes(), 0);
    }
}
