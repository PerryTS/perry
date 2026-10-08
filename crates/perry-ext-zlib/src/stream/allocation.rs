//! Codec working memory. Brotli's state and every brotli allocation, and the
//! inflate state (window and tables), live in the payload's `PayloadBuffer`s,
//! counted exactly by its `BufferOwner`; brotli reaches them through the
//! owner's C allocator hook. zstd's contexts report `sizeof`. The deflate
//! compressor boxes its arrays inside miniz_oxide, so its fixed workspace is
//! accounted from the pinned backend's actual boxed sizes.
use brotli::{Allocator, SliceWrapper, SliceWrapperMut};
use perry_ffi::native_payload::buffer::{BufferOwner, CAllocHook, PayloadBuffer, ALIGN};
use std::any::TypeId;
use std::marker::PhantomData;
use std::ptr::NonNull;

/// Brotli's allocator: its custom allocator hook, bound to the payload's
/// owner. Every clone keeps the owner (and so the hook's `opaque`) alive.
#[derive(Clone)]
pub(super) struct BufferAlloc {
    hook: CAllocHook,
    _owner: BufferOwner,
}
impl BufferAlloc {
    pub(super) fn new(owner: &BufferOwner) -> Self {
        Self {
            hook: owner.hook(),
            _owner: owner.clone(),
        }
    }
}

/// One brotli block: freed through the hook when brotli frees it, or on drop.
pub(super) struct HookBlock<T> {
    ptr: NonNull<T>,
    len: usize,
    hook: Option<CAllocHook>,
}
impl<T> Default for HookBlock<T> {
    fn default() -> Self {
        Self {
            ptr: NonNull::dangling(),
            len: 0,
            hook: None,
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
        if let Some(hook) = self.hook {
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

/// Types whose default is all-zero bytes: the hook's zeroed block already
/// holds `len` defaults, and its untouched pages stay uncommitted (as std's
/// `vec![0; n]` does). Every other type is written element by element.
fn zero_is_default<T: 'static>() -> bool {
    [
        TypeId::of::<u8>(),
        TypeId::of::<u16>(),
        TypeId::of::<u32>(),
        TypeId::of::<u64>(),
        TypeId::of::<i8>(),
        TypeId::of::<i16>(),
        TypeId::of::<i32>(),
        TypeId::of::<i64>(),
        TypeId::of::<usize>(),
        TypeId::of::<f32>(),
        TypeId::of::<f64>(),
    ]
    .contains(&TypeId::of::<T>())
}

impl<T: Default + Clone + 'static> Allocator<T> for BufferAlloc {
    type AllocatedMemory = HookBlock<T>;
    fn alloc_cell(&mut self, len: usize) -> HookBlock<T> {
        assert!(std::mem::align_of::<T>() <= ALIGN);
        let size = std::mem::size_of::<T>();
        if len == 0 || size == 0 {
            return HookBlock {
                len: if size == 0 { len } else { 0 },
                ..HookBlock::default()
            };
        }
        let Some(bytes) = len.checked_mul(size) else {
            // brotli reports an empty block as its allocation failure.
            return HookBlock::default();
        };
        let ptr = unsafe { (self.hook.alloc_func)(self.hook.opaque, bytes) } as *mut T;
        let Some(ptr) = NonNull::new(ptr) else {
            return HookBlock::default();
        };
        if !zero_is_default::<T>() {
            for i in 0..len {
                unsafe { ptr.as_ptr().add(i).write(T::default()) };
            }
        }
        HookBlock {
            ptr,
            len,
            hook: Some(self.hook),
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
    pub(super) fn new(owner: &BufferOwner, value: T) -> Self {
        assert!(std::mem::align_of::<T>() <= ALIGN);
        let mut buffer = PayloadBuffer::alloc(owner, std::mem::size_of::<T>())
            .expect("payload buffer for codec state");
        unsafe { buffer.as_mut_ptr().cast::<T>().write(value) };
        Self {
            buffer,
            _type: PhantomData,
        }
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
