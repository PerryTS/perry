//! HOSTSURF §3.2: the realm's host prototype registry, the run-time half of a
//! proven host site.
//!
//! A native provider (feather) registers each prototype object it installs
//! at realm init: `perry_host_register_prototype_v1(provider_id,
//! object_index, prototype)`, with the pair its host-surface manifest
//! declares (`provider_id`, the object's index in `objects`). Perry keeps the
//! object per realm as it keeps the realm's own intrinsics
//! (`array::prototype_addr::RealmIntrinsics`: the same cells, the same root
//! scan). The registry holds the prototype objects, never copies of their
//! members: a proven site reads the member from the object's own slot
//! ([`crate::object::field_get_set::perry_host_own_member_v1`]), so the shape
//! stays the truth.

/// Register this realm's prototype object `(provider_id, object_index)`.
/// `prototype` is the object, NaN-boxed. Called once per installed prototype
/// by the provider's installer; a second registration of the slot with
/// another object is a provider bug (debug assertion only).
#[no_mangle]
pub extern "C" fn perry_host_register_prototype_v1(
    provider_id: u32,
    object_index: u32,
    prototype: u64,
) {
    let value = crate::JSValue::from_bits(prototype);
    debug_assert!(
        value.is_pointer(),
        "perry_host_register_prototype_v1({provider_id}, {object_index}): not an object"
    );
    let addr = value.as_pointer::<u8>() as usize;
    crate::array::note_host_prototype(provider_id, object_index, addr);
}

/// This realm's registered prototype `(provider_id, object_index)`,
/// NaN-boxed: the registry load a proven host site makes (HOSTSURF §3.2).
/// The proof covers registration (the provider registers every prototype its
/// manifest declares, at realm init, before any program code runs); an
/// unregistered slot is a provider bug (debug assertion only).
#[no_mangle]
pub extern "C" fn perry_host_prototype_v1(provider_id: u32, object_index: u32) -> f64 {
    let addr = crate::array::host_prototype_addr(provider_id, object_index);
    debug_assert!(
        addr != 0,
        "host prototype ({provider_id}, {object_index}) was never registered"
    );
    crate::value::js_nanbox_pointer(addr as i64)
}
