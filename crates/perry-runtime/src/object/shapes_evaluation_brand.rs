//! The birth evaluation is a shape fact independent of [[Prototype]].
//!
//! The existing private-brand list reserves bit 62 for the evaluation's
//! holder identity. Template brands occupy u32; installed fresh brands use
//! bit 63. The holder is the class object itself. Its serial word is traced by every carrier,
//! even after Object.setPrototypeOf changes the receiver's prototype.
use super::*;
use crate::codegen_abi::LayoutWord;

pub(crate) const EVALUATION_HOLDER_BRAND: u64 = 1 << 62;

pub(super) fn is_evaluation_holder_brand(brand: u64) -> bool {
    brand & (3 << 62) == EVALUATION_HOLDER_BRAND
}

fn holder_identity(brands: &[u64]) -> Option<u64> {
    let at = brands.partition_point(|&brand| brand < EVALUATION_HOLDER_BRAND);
    let brand = *brands.get(at)?;
    is_evaluation_holder_brand(brand).then_some(brand & !EVALUATION_HOLDER_BRAND)
}

impl ShapeDescriptor {
    /// This shape has lexical evaluation ownership, separate from installed
    /// private elements and from the mutable prototype link.
    #[inline]
    pub(crate) fn has_evaluation_brand(&self) -> bool {
        holder_identity(self.brands()).is_some()
    }
}

/// The existing stable identity word, emitted as an independent shape edge.
#[inline]
pub(crate) fn evaluation_holder_slot(
    record: Option<ShapeRecordRef>,
    full: bool,
) -> Option<*mut u64> {
    // Reuse the existing test-only shape-edge mutation control; no
    // production state or extra diagnostic knob is introduced.
    #[cfg(test)]
    if test_keys_edge_suppressed() {
        return None;
    }
    let record = record?;
    let brands = unsafe { (*record.0.as_ptr()).brands() };
    shapes_prototype::identity_edge_slot(holder_identity(brands)?, full)
}

/// No prototype walk: the shape names this evaluation's shared holder.
pub(crate) unsafe fn object_evaluation_brand(obj: *const crate::object::ObjectHeader) -> u64 {
    // An instance on its class family's own layout word keeps its evaluation
    // holder in its metadata (`stamp_evaluation_holder`); one on a class
    // evaluation's copy of the layout carries it in its shape, as every
    // other receiver does.
    let meta = (*obj).meta;
    if !meta.is_null()
        && (*meta).private_evaluation_brand != 0
        && crate::object::layout_desc::is_family_object(obj)
    {
        return (*meta).private_evaluation_brand;
    }
    let Some(identity) = shape_brands_for_word(object_shape_stamp(obj)).and_then(holder_identity)
    else {
        return 0;
    };
    let holder = crate::JSValue::from_bits(identity_prototype_word(identity));
    if !holder.is_pointer() {
        return 0;
    }
    holder.bits()
}

/// May an instance of class family layout `family` be born directly on
/// ShapeId `copy`, which a class evaluation's class object names for its
/// instances (`InternalKey::EvaluationInstanceLayout`)? Yes when `copy` is a
/// present record of this agent that is an evaluation copy of `family`
/// (`evaluation_family_desc_index_plus_one`) and the identity words of its prototype and of
/// its evaluation holder still name their objects. ShapeIds are never
/// reused, so a present `copy` has the facts it had when the class object
/// recorded it: the evaluation's prototype and brand. Returns the prototype
/// and holder words, which the birth shades as the ordinary link and brand
/// stamp do. One directory read.
pub(crate) fn evaluation_copy_birth_edges(copy: LayoutWord, family: u32) -> Option<(u64, u64)> {
    let record = ShapeSlab::agent_record_for_word_present(copy)?;
    // SAFETY: a present record of this agent, read immediately.
    let record = unsafe { &*record };
    if family == 0
        || unsafe {
            crate::object::layout_desc::evaluation_layout_desc(record.family_desc_index_plus_one)
        }
        .map(|d| d.layout_id)
            != Some(family)
    {
        return None;
    }
    let prototype = identity_prototype_word(record.proto_id);
    let holder = identity_prototype_word(holder_identity(record.brands())?);
    (prototype != 0 && holder != 0).then_some((prototype, holder))
}

/// Called while a new class instance is still in its linked birth layout.
/// Subsequent transitions preserve the identity through receiver_facts.
pub(crate) unsafe fn stamp_evaluation_holder(obj: *mut crate::object::ObjectHeader, holder: u64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_raw_mut_ptr(obj);
    let holder = scope.root_nanbox_u64(holder);
    // A family instance on its family's own layout word keeps that word
    // (`layout_desc::family_store`), so the holder goes to its metadata. On a
    // class evaluation's copy of the layout the brand is a shape fact like
    // every receiver's: the brand transition keeps the copy a copy
    // (`transition_object_shape_add_brand`), and the evaluation's later
    // instances are born on the branded copy (`construct_class_object_instance`).
    if crate::object::layout_desc::is_family_layout_word(
        crate::object::layout_desc::layout_id_of_word((*obj).parent_class_id),
    ) {
        let (meta, _) = receiver.across_mut::<crate::object::ObjectHeader, _>(|| {
            // A1_UNPROVEN: tested by the layout word (is_family_layout_word), not the type byte (kind-A/B question for S0b-S0e).
            crate::object::object_meta_ensure(
                crate::object::MetaObj::runtime_born(obj),
                crate::object::MetaReason::EvalBrand,
            )
        });
        (*meta).private_evaluation_brand = holder.get_nanbox_u64();
        crate::gc::runtime_write_barrier_slot(
            meta as usize,
            std::ptr::addr_of!((*meta).private_evaluation_brand) as usize,
            (*meta).private_evaluation_brand,
        );
        return;
    }
    let brand = prepare_evaluation_holder(holder.get_nanbox_u64());
    receiver.with_mut_ptr::<crate::object::ObjectHeader, _>(|receiver| {
        transition_object_shape_add_brand(receiver, brand);
    });
}

/// The evaluation holder brand of class object `holder` (NaN-boxed), with
/// its identity word written and shaded: what a shape naming it needs
/// before any receiver carries it. Can allocate (the holder's identity
/// serial); the caller keeps its own receiver rooted.
pub(crate) unsafe fn prepare_evaluation_holder(holder: u64) -> u64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let holder = scope.root_nanbox_u64(holder);
    let class = crate::JSValue::from_bits(holder.get_nanbox_u64())
        .as_pointer::<crate::object::ObjectHeader>();
    let identity = crate::object::proto_validity::ensure_object_identity_serial(
        class as *mut crate::object::ObjectHeader,
    );
    assert!(
        identity < EVALUATION_HOLDER_BRAND,
        "evaluation holder identity exhausted"
    );
    write_identity_word(identity, holder.get_nanbox_u64());
    crate::gc::runtime_shade_external_edge(holder.get_nanbox_u64());
    EVALUATION_HOLDER_BRAND | identity
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn holder(cid: u32) -> (u64, u64) {
        let class = crate::object::js_object_alloc(cid, 0);
        crate::object::js_object_mark_class(class as i64);
        let brand = crate::value::js_nanbox_pointer(class as i64).to_bits();
        (brand, brand)
    }

    #[test]
    fn birth_shapes_share_one_evaluation_and_distinguish_repeated_evaluations() {
        let _no_gc = crate::gc::GcSuppressScope::new();
        unsafe {
            const CID: u32 = 62_641;
            let (holder_a, brand_a) = holder(CID);
            let (holder_b, brand_b) = holder(CID);
            let a = crate::object::js_object_alloc(CID, 2);
            let sibling = crate::object::js_object_alloc(CID, 2);
            let b = crate::object::js_object_alloc(CID, 2);
            let keys = crate::object::object_keys(a).arr();
            let width = crate::object::object_live_slot_count(a);
            stamp_evaluation_holder(a, holder_a);
            stamp_evaluation_holder(sibling, holder_a);
            stamp_evaluation_holder(b, holder_b);
            assert_eq!(object_shape_stamp(a), object_shape_stamp(sibling));
            assert_ne!(object_shape_stamp(a), object_shape_stamp(b));
            assert_eq!(object_evaluation_brand(a), brand_a);
            assert_eq!(object_evaluation_brand(b), brand_b);
            // A return override can stamp even the class object itself. Birth
            // ownership names its identity, never its mutable metadata brand.
            crate::object::stamp_private_evaluation_brand(
                crate::JSValue::from_bits(holder_a).as_pointer::<crate::ObjectHeader>()
                    as *mut crate::ObjectHeader,
                f64::from_bits(brand_b),
            );
            assert_eq!(object_evaluation_brand(sibling), brand_a);
            let class = crate::JSValue::from_bits(holder_a).as_pointer::<crate::ObjectHeader>();
            assert_eq!(
                (*(*class).meta).flags & crate::object::OBJECT_META_FLAG_IS_PROTOTYPE,
                0
            );
            assert_eq!(crate::object::object_keys(a).arr(), keys);
            assert_eq!(crate::object::object_live_slot_count(a), width);
            crate::object::stamp_private_evaluation_brand(a, f64::from_bits(brand_a));
            assert!((*a).meta.is_null());
            assert!((*sibling).meta.is_null());
            assert!((*b).meta.is_null());
            // An exceptional receiver can later return to its birth owner.
            // Its previous metadata override must not defeat that stamp.
            crate::object::stamp_private_evaluation_brand(a, f64::from_bits(brand_b));
            crate::object::stamp_private_evaluation_brand(a, f64::from_bits(brand_a));
            assert_eq!((*(*a).meta).private_evaluation_brand, brand_a);
            assert_eq!(object_evaluation_brand(sibling), brand_a);
            let plain = crate::object::js_object_alloc(0, 2);
            assert_eq!(object_evaluation_brand(plain), 0);
            // A later static class-env stamp replaces an ancestor's lexical
            // evaluation while keeping every installed private brand.
            transition_object_shape_add_brand(a, 1 << 63 | 77);
            stamp_evaluation_holder(a, holder_b);
            assert_eq!(object_evaluation_brand(a), brand_b);
            assert!(object_has_brand(a, 1 << 63 | 77));
            assert_eq!(
                shape_brands_for_word(object_shape_stamp(a)).unwrap().len(),
                2
            );
            // Constructor-returned objects keep their own metadata authority.
            crate::object::stamp_private_evaluation_brand(plain, f64::from_bits(brand_b));
            assert!(!(*plain).meta.is_null());
            assert_eq!(
                crate::object::private_evaluation_brand_value(crate::value::js_nanbox_pointer(
                    plain as i64
                ))
                .unwrap()
                .to_bits(),
                brand_b
            );
        }
    }

    #[test]
    fn evaluation_holder_edge_negative_control() {
        let _no_gc = crate::gc::GcSuppressScope::new();
        unsafe {
            let (holder, brand) = holder(62_642);
            let obj = crate::object::js_object_alloc(62_642, 2);
            stamp_evaluation_holder(obj, holder);
            transition_object_shape_prototype(obj, PROTO_ID_NULL, crate::value::TAG_NULL);
            let edge = evaluation_holder_slot(object_shape_record(obj), false).unwrap() as usize;
            let slots = crate::gc::test_gc_rewrite_slot_addresses(obj as usize).unwrap();
            assert!(
                slots.contains(&edge),
                "the receiver must enumerate its evaluation holder after a prototype edit"
            );
            assert_eq!(object_evaluation_brand(obj), brand);
            let _control = TestKeysEdgeSuppression::without_descriptor_edge();
            let slots = crate::gc::test_gc_rewrite_slot_addresses(obj as usize).unwrap();
            assert!(
                !slots.contains(&edge),
                "negative control must omit the holder edge in the actual walker"
            );
        }
    }
}
