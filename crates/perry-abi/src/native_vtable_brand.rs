//! The native-module vtable as a shape fact.
//!
//! A native namespace object (`NATIVE_MODULE_CLASS_ID`) carries the vtable of
//! its module on its SHAPE, as one entry of the shape record's brand list:
//! `NATIVE_VTABLE_BRAND | address`. A brand is part of shape identity and is
//! carried by every transition, so two namespaces with the same keys but
//! different modules have different shapes, and the vtable survives
//! defineProperty, delete, preventExtensions, seal and freeze.
//!
//! The band `[1 << 60, 1 << 61)` (top nibble 1) is produced by nothing else:
//! template brands are u32, the Array exotic brand is `(1 << 61) | 1`, the
//! evaluation holder is bit 62, fresh private brands bit 63, and NaN-boxed
//! values (the private evaluation brand) have top nibble 7 or F. A static
//! address is below `1 << 48`, so `(1 << 60) | address` is unambiguous.
//!
//! This is the one definition: the runtime birth and the image-row
//! registration produce the brand with [`native_vtable_brand`], every reader
//! decodes it with [`native_vtable_of`]. The brand is not a heap pointer: no
//! collector traces it (only the bit-62 holder is a traced edge).

/// The brand band of a native-module vtable address.
pub const NATIVE_VTABLE_BRAND: u64 = 1 << 60;

/// The brand that records vtable `address` on a shape.
#[inline]
pub const fn native_vtable_brand(address: usize) -> u64 {
    let address = address as u64;
    debug_assert!(address != 0 && address < (1 << 48));
    NATIVE_VTABLE_BRAND | address
}

/// Is `brand` a native-module vtable brand?
#[inline]
pub const fn is_native_vtable_brand(brand: u64) -> bool {
    brand >> 60 == 1
}

/// The vtable address a sorted brand list records, if any.
#[inline]
pub fn native_vtable_of(brands: &[u64]) -> Option<usize> {
    let at = brands.partition_point(|&brand| brand < NATIVE_VTABLE_BRAND);
    let brand = *brands.get(at)?;
    is_native_vtable_brand(brand).then_some((brand & !NATIVE_VTABLE_BRAND) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_ignores_other_bands() {
        let vt = 0x5555_1234_5678usize;
        let brand = native_vtable_brand(vt);
        assert!(is_native_vtable_brand(brand));
        assert_eq!(
            native_vtable_of(&[7, brand, (1 << 61) | 1, 1 << 62]),
            Some(vt)
        );
        assert_eq!(
            native_vtable_of(&[7, (1 << 61) | 1, 1 << 62, 1 << 63]),
            None
        );
        assert_eq!(native_vtable_of(&[]), None);
        assert!(!is_native_vtable_brand(0x7FFD_0000_0000_0001));
        assert!(!is_native_vtable_brand(u64::MAX - 2));
    }
}
