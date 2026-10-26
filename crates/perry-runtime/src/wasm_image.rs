//! Linear-memory counterparts of the native image sections.
//!
//! Rust's Wasm custom sections cannot contain relocations. Keep the actual
//! records in linear memory and enumerate their addresses through constructors
//! instead. WASI's command/reactor initialization runs these before JS starts.
//! These are immutable image addresses, never pointers into the managed heap.
//!
//! This covers the runtime's own images, which live outside the `perry_strings`
//! section. Program string literals are inside it, and `value::addr_class`
//! admits them by the linker-synthesized `__start_/__stop_perry_strings` bounds.

#[cfg(target_family = "wasm")]
mod records {
    pub(crate) struct Atom(pub *const crate::object::layout_desc::StaticAtom);
    pub(crate) struct Layout(pub *mut crate::object::layout_desc::PerryLayoutDesc);
    pub(crate) struct Image {
        pub start: *const u8,
        pub len: usize,
    }
    // The pointed-to image outlives the instance. Layout ids are published using
    // the same atomic registration protocol as native image descriptors.
    unsafe impl Sync for Atom {}
    unsafe impl Sync for Layout {}
    unsafe impl Sync for Image {}
    inventory::collect!(Atom);
    inventory::collect!(Layout);
    inventory::collect!(Image);

    /// Return the exact static allocation containing the candidate string header.
    /// Never treat the gaps between static allocations as readable image records.
    pub(crate) fn bounds(addr: usize) -> Option<(usize, usize)> {
        static RANGES: std::sync::OnceLock<Box<[(usize, usize)]>> = std::sync::OnceLock::new();
        let ranges = RANGES.get_or_init(|| {
            let mut ranges: Vec<_> = inventory::iter::<Image>
                .into_iter()
                .map(|image| (image.start as usize, image.start as usize + image.len))
                .collect();
            ranges.sort_unstable();
            ranges.dedup();
            ranges.into_boxed_slice()
        });
        let index = ranges
            .partition_point(|&(start, _)| start <= addr)
            .checked_sub(1)?;
        let range = ranges[index];
        (addr < range.1).then_some(range)
    }
}
#[cfg(target_family = "wasm")]
pub(crate) use records::*;

macro_rules! wasm_atom {
    ($name:ident) => {
        #[cfg(target_family = "wasm")]
        inventory::submit! { crate::wasm_image::Atom(std::ptr::addr_of!($name)) }
    };
}
macro_rules! wasm_layout {
    ($name:ident) => {
        #[cfg(target_family = "wasm")]
        inventory::submit! { crate::wasm_image::Layout(std::ptr::addr_of_mut!($name)) }
    };
}
macro_rules! wasm_string_image {
    ($name:ident) => {
        #[cfg(target_family = "wasm")]
        inventory::submit! {
            crate::wasm_image::Image {
                start: std::ptr::addr_of!($name).cast(),
                len: std::mem::size_of_val(&$name),
            }
        }
    };
}
