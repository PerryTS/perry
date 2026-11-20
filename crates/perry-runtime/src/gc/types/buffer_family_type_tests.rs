use super::*;

/// #10694: every buffer-family flavor is the same GC kind under another
/// brand. If one ever diverged (a different descriptor, finalize hook or
/// movability), the brand would change how the collector treats the cell.
#[test]
fn buffer_family_flavors_share_one_gc_kind() {
    let base = *gc_type_info(GC_TYPE_BUFFER).expect("buffer type info");
    let mut flavors = 0;
    for t in 0..=u8::MAX {
        let Some(info) = gc_type_info(t) else {
            assert!(!is_byte_family_type(t), "flavor {t} has no type info");
            continue;
        };
        assert_eq!(info.type_id, t, "GC_TYPE_INFO_BY_ID is indexed by type id");
        let same_kind = GcTypeInfo {
            type_id: base.type_id,
            name: base.name,
            ..*info
        } == base;
        assert_eq!(
            is_byte_family_type(t),
            same_kind,
            "type {t} ({}) must be a buffer flavor exactly when it is GC_TYPE_BUFFER's kind",
            info.name
        );
        flavors += usize::from(is_byte_family_type(t));
    }
    assert_eq!(flavors, 38);
    for t in [
        GC_TYPE_BUFFER_UINT8ARRAY,
        GC_TYPE_BUFFER_SECRET_KEY,
        GC_TYPE_BUFFER_CRYPTO_KEY,
    ] {
        assert!(is_uint8array_buffer_type(t));
    }
    assert!(!is_uint8array_buffer_type(GC_TYPE_BUFFER));
    assert!(!is_uint8array_buffer_type(GC_TYPE_BUFFER_ARRAY_BUFFER));
}
