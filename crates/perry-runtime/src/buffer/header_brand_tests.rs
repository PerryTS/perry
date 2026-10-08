//! #10694: a buffer's flavor is its GC type byte. These pin the properties the
//! old address-keyed registries could not give, and the guards the header read
//! needs that a table lookup did not.

use super::header::*;
use crate::gc::{
    GC_TYPE_BUFFER, GC_TYPE_BUFFER_ARRAY_BUFFER, GC_TYPE_BUFFER_CRYPTO_KEY,
    GC_TYPE_BUFFER_SECRET_KEY, GC_TYPE_BUFFER_UINT8ARRAY,
};

fn brand(addr: usize) -> Option<u8> {
    buffer_family_type(addr)
}

/// A brand written on one thread is read on another with no publication step:
/// it is in the cell. The old design needed process-global `EXTERNAL_*` sets
/// (and a latch that every inserter had to arm, #9176) for exactly this case,
/// because perry-stdlib produces branded bytes off the main thread.
#[test]
fn a_brand_written_on_one_thread_is_seen_on_another() {
    let u8a = super::store::alloc_test(GC_TYPE_BUFFER_UINT8ARRAY, 8) as usize;
    let key = super::store::alloc_test(GC_TYPE_BUFFER_CRYPTO_KEY, 32) as usize;
    std::thread::spawn(move || {
        js_buffer_set_crypto_key_meta_external(key, 1, 2, 1, 1, 0, 256);
    })
    .join()
    .expect("marking thread");
    assert!(is_uint8array_buffer(u8a));
    assert_eq!(brand(u8a), Some(GC_TYPE_BUFFER_UINT8ARRAY));
    assert_eq!(brand(key), Some(GC_TYPE_BUFFER_CRYPTO_KEY));
    assert!(
        is_uint8array_buffer(key),
        "a CryptoKey's storage is Uint8Array-backed"
    );
    assert_eq!(
        crypto_key_meta(key).map(|m| (m.0, m.1, m.5)),
        Some((1, 2, 256))
    );
    let seen_elsewhere = std::thread::spawn(move || (is_uint8array_buffer(u8a), brand(key)))
        .join()
        .expect("probe thread");
    assert_eq!(seen_elsewhere, (true, Some(GC_TYPE_BUFFER_CRYPTO_KEY)));
}

/// Branding is a statement about a cell that is already a buffer. A "mark" on
/// anything else is refused, so no address can be made to answer as a buffer
/// by a stray registration (the forged-registry tests this replaces needed a
/// separate header validation for that).
#[test]
fn a_brand_cannot_be_written_onto_a_non_buffer_cell() {
    let obj = crate::object::js_object_alloc(0, 2) as usize;

    set_crypto_key_meta(obj, 1, 2, 1);
    assert_eq!(brand(obj), None);
    assert!(!is_registered_buffer(obj));
    assert_eq!(crypto_key_meta(obj), None);
    let header =
        unsafe { crate::value::addr_class::try_read_gc_header(obj) }.expect("object header");
    assert_eq!(
        header.obj_type,
        crate::gc::GC_TYPE_OBJECT,
        "the object kept its own type"
    );
}

/// SecretKey and CryptoKey cells are born with their specific brands. Metadata
/// registration keeps that brand and refuses an ordinary Buffer.
#[test]
fn key_brands_are_final_at_birth() {
    let secret = super::store::alloc_test(GC_TYPE_BUFFER_SECRET_KEY, 16) as usize;

    assert_eq!(brand(secret), Some(GC_TYPE_BUFFER_SECRET_KEY));
    assert!(is_secret_key(secret));

    let crypto = super::store::alloc_test(GC_TYPE_BUFFER_CRYPTO_KEY, 16) as usize;
    set_crypto_key_meta(crypto, 1, 2, 1);

    assert_eq!(brand(crypto), Some(GC_TYPE_BUFFER_CRYPTO_KEY));
    assert!(crypto_key_meta(crypto).is_some());

    let plain = buffer_alloc(16) as usize;
    assert_eq!(
        brand(plain),
        Some(GC_TYPE_BUFFER),
        "buffer_alloc births a Node Buffer"
    );

    let array_buffer = super::store::alloc_test(GC_TYPE_BUFFER_ARRAY_BUFFER, 16) as usize;
    assert_eq!(brand(array_buffer), Some(GC_TYPE_BUFFER_ARRAY_BUFFER));
    set_crypto_key_meta(plain, 1, 2, 1);
    assert_eq!(brand(plain), Some(GC_TYPE_BUFFER));
    assert!(crypto_key_meta(plain).is_none());
}

/// Persistent symbols have an honest GC_TYPE_SYMBOL prefix. Brand probes
/// consult that prefix even when payload bytes resemble a BufferHeader.
#[test]
fn a_persistent_symbol_is_rejected_by_its_header_brand() {
    let symbol = crate::symbol::well_known_symbol("iterator");
    let addr = symbol as usize;
    let header = unsafe { crate::gc::header_from_trusted_user_ptr(symbol.cast()) };
    assert_eq!(unsafe { (*header).obj_type }, crate::gc::GC_TYPE_SYMBOL);
    assert!(!is_registered_buffer(addr));
    assert!(!is_uint8array_buffer(addr));
    assert_eq!(crate::typedarray::lookup_typed_array_kind(addr), None);
}

/// A real buffer's payload is never used to screen its header brand.
#[test]
fn a_buffer_whose_length_word_equals_the_symbol_magic_is_still_a_buffer() {
    let buf = buffer_alloc(8);
    let saved = unsafe { super::store::length(buf as usize) as u32 };
    unsafe {
        super::store::set_length(buf as usize, crate::symbol::SYMBOL_MAGIC);
    }
    let seen = is_registered_buffer(buf as usize);
    unsafe {
        super::store::set_length(buf as usize, saved);
    }
    assert!(seen);
}
