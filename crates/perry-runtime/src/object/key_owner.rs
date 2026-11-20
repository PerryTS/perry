//! The owner of a key set, as the subsystems beyond the Table see it (lane
//! DYNP FR6, DESIGN_FR step 1).
//!
//! The Table operations of `overflow_home` take the owner's address and its
//! home kind. The subsystems that read more of an owner than its Table
//! take the same pair: integrity (the sealed-intrinsic lock gate,
//! `builtin_proto`), the Set/Map/WeakMap key identity (`set::index`) and the
//! instanceof `@@hasInstance` proofs (`instanceof::dynamic_dispatch`). Each
//! asks the owner, through [`ObjHome`], for what it needs: its meta, its layout's
//! no-symbol proof, its own symbol keys. [`ObjHome`] (an `ObjectHeader`) is
//! the one owner kind here; a function's, array's or exotic cell's property
//! holder is handed in as the object it is. Compile-time generic: no owner
//! kind is tested at run time.
use crate::object::overflow::home::ObjHome;
use crate::object::{MetaReason, ObjectHeader, ObjectMeta};

impl ObjHome {
    /// Descriptor leaves (the pipeline of `descriptor_state` writes the
    /// owner's keys only through these). Record `edits` on the owner: its
    /// descriptor mark and its keys' attribute entries.
    #[inline(always)]
    pub(crate) unsafe fn note_descriptors(
        owner: usize,
        edits: &[crate::object::key_attrs::AttrsEdit<'_>],
    ) {
        // A write: prove the owner is a GC allocation before touching its
        // header or keys (`attrs_live_in_keys_for_install`).
        if let Some(header) = crate::value::addr_class::try_read_tracked_gc_header(owner) {
            let header = header.as_ptr();
            if crate::gc_type_is_object!((*header).obj_type) {
                // The header bit mirrors the resulting shape: "some key entry
                // is non-default". The edits decide it: an install whose
                // attributes are all default (`writable`, `enumerable` and
                // `configurable` all true) leaves the shape a plain key add
                // and sets no bit; a bit an earlier entry needed is set
                // already. An empty edit list is a birth whose keys carry
                // their attributes (`note_attrs_born_with_keys`).
                if edits.is_empty() || edits.iter().any(|e| !e.leaves_default()) {
                    (*header)._reserved |= crate::gc::OBJ_FLAG_HAS_DESCRIPTORS;
                }
                crate::object::key_attrs::apply_edits(owner as *mut ObjectHeader, edits);
            }
        }
    }
    /// The accessor pair own key `key` holds.
    #[inline(always)]
    pub(crate) unsafe fn own_accessor_pair(
        owner: usize,
        key: &[u8],
    ) -> Option<crate::object::accessor_pair::Accessor> {
        crate::object::accessor_pair::own_accessor(owner, key)
    }
    /// Store (or, with `None`, clear) own key `key`'s accessor pair.
    #[inline(always)]
    pub(crate) unsafe fn store_accessor_pair(
        owner: usize,
        key: &[u8],
        pair: Option<crate::object::accessor_pair::Accessor>,
    ) {
        crate::object::accessor_pair::store_own_accessor_bytes(owner, key, pair)
    }
    /// Do the owner's attributes and accessor pairs live with its keys?
    #[inline(always)]
    pub(crate) unsafe fn attrs_live_in_keys(owner: usize) -> bool {
        crate::object::key_attrs::attrs_live_in_keys_for_install(owner)
    }
    /// The owner's attribute summary: `key_attrs::SUMMARY_*` over every own
    /// key, 0 when every key is a default data property.
    #[inline(always)]
    pub(crate) unsafe fn key_summary(owner: usize) -> u8 {
        crate::object::key_attrs::object_home_summary(owner as *const ObjectHeader)
    }
    /// The attribute entry (`key_attrs::ENTRY_*`) of own key `key`: 0 when the
    /// key is default or absent. `accessor`: only `ENTRY_ACCESSOR` matters, so
    /// the owner may answer from its accessor filter alone.
    #[inline(always)]
    pub(crate) unsafe fn entry_of(owner: usize, key: &[u8], accessor: bool) -> u8 {
        crate::object::key_attrs::object_home_entry(owner as *const ObjectHeader, key, accessor)
    }
    /// May `edits` reach the owner's keys at all (an ordinary object: it is
    /// shaped)?
    #[inline(always)]
    pub(crate) unsafe fn keys_editable(owner: usize) -> bool {
        crate::object::key_attrs::object_home_keys_editable(owner as *mut ObjectHeader)
    }
    /// Is `key` own (a private name with `private`)?
    #[inline(always)]
    pub(crate) unsafe fn key_present(owner: usize, key: &[u8], private: bool) -> bool {
        crate::object::key_attrs::object_home_key_present(owner as *mut ObjectHeader, key, private)
    }
    /// Claim absent `key` with attribute entry `entry` (non-default).
    #[inline(always)]
    pub(crate) unsafe fn claim_absent(owner: usize, key: &[u8], entry: u8) {
        crate::object::key_attrs::object_home_claim_absent(owner as *mut ObjectHeader, key, entry)
    }
    /// Fold `edits` into the owner's present keys.
    #[inline(always)]
    pub(crate) unsafe fn edit_present(
        owner: usize,
        edits: &[crate::object::key_attrs::AttrsEdit<'_>],
    ) {
        crate::object::key_attrs::object_home_edit_present(owner as *mut ObjectHeader, edits)
    }
    /// The owner's own accessor keys, in key order.
    #[inline(always)]
    pub(crate) unsafe fn accessor_names(owner: usize) -> Vec<String> {
        crate::object::key_attrs::object_home_accessor_names(owner as *const ObjectHeader)
    }
    /// Is `key` an own private element of `owner` with no equally spelled
    /// public key? (The caller has checked `SUMMARY_PRIVATE`.)
    #[inline(always)]
    pub(crate) unsafe fn private_only(owner: usize, key: &[u8]) -> bool {
        crate::object::key_attrs::object_home_private_only(owner as *const ObjectHeader, key)
    }
    /// Is own private element `key` present on `owner`? (The caller has
    /// checked `SUMMARY_PRIVATE`.)
    #[inline(always)]
    pub(crate) unsafe fn private_present(owner: usize, key: &[u8]) -> bool {
        crate::object::key_attrs::object_home_private_present(owner as *const ObjectHeader, key)
    }
    /// The storage slot of private element `key` when `owner` holds it as
    /// optional (present per object), else `None`. (The caller has checked
    /// `SUMMARY_PRIVATE`.)
    #[inline(always)]
    pub(crate) unsafe fn optional_private_slot(owner: usize, key: &[u8]) -> Option<u32> {
        crate::object::key_attrs::object_home_optional_private_slot(
            owner as *const ObjectHeader,
            key,
        )
    }
    /// The owner's meta, or null.
    #[inline(always)]
    pub(crate) unsafe fn meta(owner: usize) -> *mut ObjectMeta {
        crate::meta_read!(crate::meta_obj_runtime_born!(owner as *mut ObjectHeader))
    }
    /// The owner's meta, made for `reason` when it has none. May allocate;
    /// the caller roots (or pins) `owner`.
    #[inline(always)]
    pub(crate) unsafe fn meta_ensure(owner: usize, reason: MetaReason) -> *mut ObjectMeta {
        crate::object::object_meta_ensure(
            crate::meta_obj_runtime_born!(owner as *mut ObjectHeader),
            reason,
        )
    }
    /// The owner's own keys are proven to hold no symbol (its layout says so
    /// and no Table holds keys past the layout).
    #[inline(always)]
    pub(crate) unsafe fn proves_no_symbols(owner: usize) -> bool {
        let obj = owner as *mut ObjectHeader;
        !crate::object::overflow::shadowed(obj)
            && crate::object::shapes::object_shape_record(obj).is_some_and(|record| {
                // A legacy dictionary can have an ordinary keyless shape;
                // its private key list carries no immutable absence proof.
                record.keys() != 0 && record.proves_no_symbols()
            })
    }
    /// The position of own symbol key `symbol`, if the owner has it.
    #[inline(always)]
    pub(crate) unsafe fn own_symbol_position(owner: usize, symbol: usize) -> Option<u32> {
        crate::object::shaped_symbols::position(owner as *const ObjectHeader, symbol)
    }
}

/// The owner's meta for a reader (`create` false: null when it has none) or a
/// writer (`create`: made for `reason`).
#[inline(always)]
pub(crate) unsafe fn meta_for(owner: usize, reason: MetaReason, create: bool) -> *mut ObjectMeta {
    if create {
        ObjHome::meta_ensure(owner, reason)
    } else {
        ObjHome::meta(owner)
    }
}
