//! Normalize descriptor writes to each owner's ordinary property holder.
//! A function additionally refreshes its receiver shape after editing its bag.
use super::ObjectHeader;
use crate::object::accessor_pair::Accessor;
use crate::object::overflow::home::ObjHome;

/// The key owner a descriptor edit writes. Each pipeline leaf dispatches on
/// it once (`leaf::<false>` here; a function's own key store is the second
/// kind, lane DYNP S2c), so the pipeline bodies stay one copy.
#[derive(Clone, Copy)]
pub(crate) enum KeyTarget {
    Object(*mut ObjectHeader),
    /// A function, addressed by its closure: its own-key store stays behind
    /// the `closure::props` leaves.
    Function(usize),
}

use crate::closure::props as fnp;

/// The integrity level an `Object.preventExtensions/seal/freeze` applies.
#[derive(Clone, Copy)]
pub(crate) enum Restrict {
    NoExtend,
    Seal,
    Freeze,
}

impl KeyTarget {
    /// The owner's meta record: read, or created when `create` (null when the
    /// owner has no holder, or has none yet and `create` is false).
    #[inline]
    pub(crate) unsafe fn meta(
        self,
        reason: crate::object::MetaReason,
        create: bool,
    ) -> *mut crate::object::ObjectMeta {
        match self {
            Self::Object(o) => Self::object_meta(o, reason, create),
            Self::Function(p) => fnp::fn_meta(p, reason, create),
        }
    }
    /// [`KeyTarget::meta`] on an ordinary holder.
    #[inline]
    pub(crate) unsafe fn object_meta(
        o: *mut ObjectHeader,
        reason: crate::object::MetaReason,
        create: bool,
    ) -> *mut crate::object::ObjectMeta {
        if o.is_null() {
            return std::ptr::null_mut();
        }
        // A1_UNPROVEN: the holder may be the user object itself (kind-A/B question for S0b-S0f).
        if create {
            crate::object::object_meta_ensure(crate::meta_obj_runtime_born!(o), reason)
        } else {
            crate::meta_read!(crate::meta_obj_runtime_born!(o))
        }
    }
    /// Apply an integrity level to the owner's own keys (no-op without a holder).
    #[inline]
    pub(crate) unsafe fn restrict(self, level: Restrict) {
        match self {
            Self::Object(o) => Self::restrict_object(o, level),
            Self::Function(p) => fnp::fn_restrict(p, level),
        }
    }
    /// Do the owner's own keys satisfy the level (true without a holder)?
    #[inline]
    pub(crate) unsafe fn satisfies(self, frozen: bool) -> bool {
        match self {
            Self::Object(o) => Self::object_satisfies(o, frozen),
            Self::Function(p) => fnp::fn_satisfies(p, frozen),
        }
    }
    /// The key's attribute entry (0: absent, or present with default attributes).
    #[inline(always)]
    pub(crate) unsafe fn key_entry(self, key: &[u8]) -> u8 {
        match self {
            Self::Object(o) if o.is_null() => 0,
            Self::Object(o) => super::super::key_attrs::object_key_entry(o, key),
            Self::Function(p) => fnp::fn_key_entry(p, key),
        }
    }
    #[inline(always)]
    pub(crate) unsafe fn key_is_accessor(self, key: &[u8]) -> bool {
        match self {
            Self::Object(o) => {
                !o.is_null() && super::super::key_attrs::object_key_is_accessor(o, key)
            }
            Self::Function(p) => fnp::fn_key_is_accessor(p, key),
        }
    }
    /// The holder shape's descriptor summary (0 when it has no keys).
    #[inline(always)]
    pub(crate) unsafe fn summary(self) -> u8 {
        match self {
            Self::Object(o) if o.is_null() => 0,
            Self::Object(o) => super::super::key_attrs::object_summary(o),
            Self::Function(p) => fnp::fn_summary(p),
        }
    }
    #[inline(always)]
    pub(crate) unsafe fn accessor_names(self) -> Vec<String> {
        match self {
            Self::Object(o) if o.is_null() => Vec::new(),
            Self::Object(o) => super::super::key_attrs::object_accessor_key_names(o),
            Self::Function(p) => fnp::fn_accessor_names(p),
        }
    }
    /// The descriptor funnel for an edit on this holder.
    #[inline(always)]
    pub(crate) fn note_edits(self, edits: &[super::AttrsEdit<'_>]) {
        match self {
            Self::Object(o) => super::note_descriptor_target_edits(o as usize, edits),
            Self::Function(p) => unsafe { fnp::fn_note_edits(p, edits) },
        }
    }
    #[inline(always)]
    pub(crate) fn accessor_replaced(self, key: &[u8], previous: Option<Accessor>, now: Accessor) {
        match self {
            Self::Object(o) => {
                super::note_accessor_function_replaced_bytes(o as usize, key, previous, now)
            }
            Self::Function(p) => unsafe { fnp::fn_accessor_replaced(p, key, previous, now) },
        }
    }
    #[inline(always)]
    pub(crate) fn note_named_mutation(self, key: &[u8]) {
        match self {
            Self::Object(o) => crate::typedarray_named::note_named_mutation(o as usize, key),
            Self::Function(p) => unsafe { fnp::fn_note_named_mutation(p, key) },
        }
    }
    /// The holder object a test inspects.
    #[cfg(test)]
    pub(crate) fn test_holder(self) -> *const ObjectHeader {
        match self {
            Self::Object(o) => o,
            Self::Function(p) => unsafe { fnp::bag_of(p) },
        }
    }
    #[inline(always)]
    pub(crate) unsafe fn note_descriptors(self, edits: &[super::AttrsEdit<'_>]) {
        match self {
            Self::Object(o) => ObjHome::note_descriptors(o as usize, edits),
            Self::Function(p) => fnp::fn_note_descriptors(p, edits),
        }
    }
    #[inline(always)]
    pub(crate) unsafe fn own_accessor_pair(self, key: &[u8]) -> Option<Accessor> {
        match self {
            Self::Object(o) => ObjHome::own_accessor_pair(o as usize, key),
            Self::Function(p) => fnp::fn_own_accessor_pair(p, key),
        }
    }
    #[inline(always)]
    pub(crate) unsafe fn store_accessor_pair(self, key: &[u8], pair: Option<Accessor>) {
        match self {
            Self::Object(o) => ObjHome::store_accessor_pair(o as usize, key, pair),
            Self::Function(p) => fnp::fn_store_accessor_pair(p, key, pair),
        }
    }
    #[inline(always)]
    pub(crate) unsafe fn attrs_live_in_keys(self) -> bool {
        match self {
            Self::Object(o) => ObjHome::attrs_live_in_keys(o as usize),
            Self::Function(p) => fnp::fn_attrs_live_in_keys(p),
        }
    }
}

pub(crate) struct HolderEdit {
    owner: usize,
    pub(super) target: KeyTarget,
    _no_move: crate::gc::GcSuppressScope,
}

impl HolderEdit {
    #[inline]
    pub(crate) fn new(owner: usize) -> Option<Self> {
        if !crate::closure::is_closure_ptr(owner) {
            let _no_move = crate::gc::GcSuppressScope::new();
            let bag = unsafe {
                if crate::value::addr_class::is_proxy_cell(owner)
                    && crate::proxy_hooks::canonical_address(f64::from_bits(
                        crate::value::POINTER_TAG | owner as u64,
                    ))
                    .is_some()
                {
                    return Some(Self {
                        owner,
                        target: KeyTarget::Object(crate::proxy_hooks::property_bag_ensure(owner)),
                        _no_move,
                    });
                }
                let header = crate::value::addr_class::try_read_tracked_gc_header(owner);
                match header.map(|h| (*h.as_ptr()).obj_type) {
                    Some(crate::gc_array_type_pat!()) => crate::array::array_property_bag_ensure(
                        owner as *mut crate::array::ArrayHeader,
                    ),
                    Some(crate::gc::GC_TYPE_LAZY_ARRAY) => {
                        let array = crate::json_tape::force_materialize_lazy(
                            owner as *mut crate::json_tape::LazyArrayHeader,
                        );
                        crate::array::array_property_bag_ensure(array)
                    }
                    Some(crate::gc::GC_TYPE_OBJECT) => {
                        let live = super::filter::resolve_object_holder(owner);
                        if live == owner {
                            return None;
                        }
                        // An evacuation alias is not a native registry handle.
                        // Normalize writes as well as reads to the live holder.
                        return Some(Self {
                            owner: live,
                            target: KeyTarget::Object(live as *mut ObjectHeader),
                            _no_move,
                        });
                    }
                    _ if crate::buffer::header::is_owned_byte_cell(owner) => {
                        crate::buffer::store::bag_ensure(owner)
                    }
                    Some(
                        crate::gc::GC_TYPE_ERROR
                        | crate::gc::GC_TYPE_MAP
                        | crate::gc::GC_TYPE_SET
                        | crate::gc::GC_TYPE_PROMISE
                        | crate::gc::GC_TYPE_DATE_CELL,
                    ) => super::super::cell_expando_ensure(owner)?,
                    Some(crate::gc::GC_TYPE_TEMPORAL) => {
                        super::super::exotic_expando::property_bag_ensure(owner)
                    }
                    None => super::super::handle_expando::handle_property_bag_ensure(owner as i64),
                    Some(kind) => {
                        debug_assert!(false, "GC cell type {kind} is not a descriptor holder");
                        return None;
                    }
                }
            };
            return Some(Self {
                owner,
                target: KeyTarget::Object(bag),
                _no_move,
            });
        }
        Some(Self::for_closure(owner))
    }

    /// The key owner an install on `obj` writes: the edit's holder, or `obj`
    /// itself when it is its own holder.
    #[inline(always)]
    pub(crate) fn target_of(edit: &Option<Self>, obj: usize) -> KeyTarget {
        edit.as_ref()
            .map_or(KeyTarget::Object(obj as *mut ObjectHeader), |e| e.target)
    }

    /// The object holder an edit on the non-function owner `obj` writes
    /// (`obj` itself when it is its own holder). A function owner's edit is
    /// [`Self::for_closure`]; its callers never reach here with one.
    #[inline(always)]
    pub(crate) fn object_holder(edit: &Option<Self>, obj: usize) -> usize {
        match edit {
            Some(Self {
                target: KeyTarget::Object(o),
                ..
            }) => *o as usize,
            _ => obj,
        }
    }

    #[inline(never)]
    pub(super) fn for_closure(owner: usize) -> Self {
        let no_move = crate::gc::GcSuppressScope::new();
        super::super::prop_plan::prop_plan_epoch_bump_for_owner(owner);
        // SAFETY: a proven live closure, held in a no-move window. The edit
        // writes the function's store, made here when it has none.
        unsafe { crate::closure::props::ensure_key_store(owner) };
        Self {
            owner,
            target: KeyTarget::Function(owner),
            _no_move: no_move,
        }
    }
    /// The function's `prototype` leaves its slot for its bag (one way: the
    /// slot stays a hole), made first when the function has not made it.
    /// Every attribute edit that is not the birth attributes comes here.
    #[inline(never)]
    pub(crate) fn prototype_leaves_slot(&self) {
        unsafe {
            if !crate::closure::is_closure_ptr(self.owner)
                || !crate::closure::props::prototype_slot_holds_key(self.owner)
            {
                return;
            }
            let value = match crate::closure::props::prototype_slot_take(self.owner) {
                Some(value) => value,
                None => {
                    let made = crate::object::class_registry::js_function_prototype_value_for_read(
                        crate::value::js_nanbox_pointer(self.owner as i64),
                    );
                    crate::closure::props::prototype_slot_take(self.owner).unwrap_or(made)
                }
            };
            crate::closure::props::bag_define_value_in_bag(self.owner, b"prototype", value);
        }
    }

    #[inline(never)]
    pub(super) fn materialize_data_keys(&self, edits: &[super::AttrsEdit<'_>]) {
        if !crate::closure::is_closure_ptr(self.owner) {
            return;
        }
        for edit in edits {
            let super::AttrsEdit::Data(key, bits) = edit else {
                continue;
            };
            let Ok(name) = std::str::from_utf8(key) else {
                continue;
            };
            unsafe {
                // Other attributes for `prototype` live with its key in the
                // bag: the slot gives the value up to it (made first when the
                // function has not made it yet).
                if key == b"prototype"
                    && !crate::closure::props::prototype_slot(self.owner).is_null()
                {
                    if *bits != crate::object::PropertyAttrs::new(true, false, false).bits {
                        self.prototype_leaves_slot();
                    }
                    continue;
                }
                if !crate::closure::props::bag_has_own(self.owner, key)
                    && super::super::has_own_helpers::closure_own_key_present(self.owner, name)
                {
                    let value = crate::closure::closure_get_dynamic_prop(self.owner, name);
                    crate::closure::props::bag_define_value(self.owner, name, value);
                }
            }
        }
    }
}

impl Drop for HolderEdit {
    fn drop(&mut self) {
        if crate::closure::is_closure_ptr(self.owner) {
            crate::closure::shape::refresh_closure_shape(self.owner);
        }
    }
}

#[cfg(test)]
#[path = "function_attrs_tests.rs"]
mod tests;
