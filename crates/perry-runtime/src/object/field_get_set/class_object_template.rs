//! A per-evaluation class object is born in its template's final shape.
//!
//! Every evaluation of one class template (`ClassExprFresh`) gives its class
//! object the same own keys, in the same order and with the same attributes:
//! `length` and `name` (or the placeholder a static field of that name stores
//! over), one function object per ClassBody static method
//! ([`define_class_object_own_properties`](super::define_class_object_own_properties)),
//! then its pinned parent when the template has heritage. So the shape the
//! class object is created in is a fact of the template, not of the
//! evaluation. The first evaluation in an agent builds it key by key through
//! the ordinary define path, and the shape it reaches is a real shape record:
//! canonical keys with their attribute entries, the class kind, the prototype
//! identity, the rep. Every later evaluation allocates its class object
//! directly in that shape and fills the slots: no birth shape, no class-kind
//! transition, no key-add transition, no by-name define, no attribute edit.
//! The captured environment, which the evaluation adds next, and the
//! evaluation's prototype object, built at its first use, are each one
//! recorded transition (`class_object_add_internal`).
//!
//! The evaluation's prototype object is born the same way, in the template's
//! final prototype shape: `constructor`, then one function object per method
//! running the method's closure-convention entry (`<method>__eclo`) at home in
//! the evaluation, linked to the evaluation's parent prototype.
//!
//! What the template remembers is ShapeIds and what fills each slot; it holds
//! nothing about any object. It remembers them in its own image record, the
//! template cell ([`TemplateCell`]) codegen emits once per template: no table
//! is keyed by the template. The shape records it names are external carriers
//! for the agent's life, so no id it names can come to name other facts.

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

/// How one slot of a template's final class-object shape is filled.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fill {
    /// The same non-pointer value in every evaluation (`length`, or the
    /// `undefined` placeholder of a static field named `length` / `name`).
    Bits(u64),
    /// The class's `name` string.
    Name,
    /// A new function object running this ClassBody static method's
    /// closure-convention entry (its `JsFunctionInfo`), at home in the class
    /// object.
    Method(usize),
    /// The evaluation's pinned parent (`__perry_parent_class`).
    Parent,
}

/// The most internal-key transitions one template records.
const MAX_TEMPLATE_EDGES: usize = 8;

/// A per-evaluation class template's own record: an image static codegen
/// emits once per template (`@perry_ctpl.<cid>`) and hands to the template's
/// evaluation site, and whose address the template's vtable entry carries
/// ([`js_register_class_template_cell`]) for the runtime paths that start from
/// a class object. Codegen gives it its length in words (word 0) and nothing
/// else; this module owns the rest of the layout.
///
/// It memoizes what the template's first evaluation reached: the final
/// ShapeIds of its class object and prototype, how each of their slots is
/// filled, and the internal-key transitions between them. ShapeIds name one
/// agent's own shape records, so the cell answers only the thread that
/// recorded it (its owner token). Any other thread takes the ordinary path,
/// which is correct and only slower.
#[derive(Clone, Copy)]
pub(crate) struct TemplateCell(*const AtomicU64);

/// The cell's length in words, written by codegen.
const W_LEN: usize = 0;
/// The token of the thread that owns the cell's ShapeIds; 0 while unclaimed.
const W_OWNER: usize = 1;
/// The class object's final ShapeId; 0 when not recorded.
const W_CLASS_SHAPE: usize = 2;
/// `field_count | static_field_mask << 32 | has_parent << 40 | fills << 48`.
const W_CLASS_FACTS: usize = 3;
/// Whether the class object's shape was settled (recorded or refused).
const W_CLASS_SETTLED: usize = 4;
/// The prototype object's final ShapeId; 0 when not recorded.
const W_PROTO_SHAPE: usize = 5;
/// `field_count | fills << 32`.
const W_PROTO_FACTS: usize = 6;
/// The [[Prototype]] identity the prototype's final shape names.
const W_PROTO_ID: usize = 7;
/// `MAX_TEMPLATE_EDGES` pairs: `from | to << 32`, `slot | key << 32` (key is
/// `InternalKey as u64 + 1`, 0 for an empty pair).
const W_EDGES: usize = 8;
/// The class fills (two words each: tag, value), then the prototype fills.
const W_FILLS: usize = W_EDGES + 2 * MAX_TEMPLATE_EDGES;

/// Fill tags.
const TAG_BITS: u64 = 1;
const TAG_NAME: u64 = 2;
const TAG_METHOD: u64 = 3;
const TAG_PARENT: u64 = 4;
const TAG_CONSTRUCTOR: u64 = 5;

#[thread_local]
static THREAD_TOKEN: std::cell::Cell<u64> = std::cell::Cell::new(0);
static NEXT_THREAD_TOKEN: AtomicU64 = AtomicU64::new(1);

/// This thread's token: unique for the process's life, never 0.
#[inline]
fn thread_token() -> u64 {
    let token = THREAD_TOKEN.get();
    if token != 0 {
        return token;
    }
    let token = NEXT_THREAD_TOKEN.fetch_add(1, Ordering::Relaxed);
    THREAD_TOKEN.set(token);
    token
}

impl TemplateCell {
    /// The cell at `ptr`, when it is one: non-null, with a length that holds
    /// its fixed words.
    pub(crate) unsafe fn from_ptr(ptr: *const u64) -> Option<Self> {
        if ptr.is_null() || (ptr as usize) % std::mem::align_of::<AtomicU64>() != 0 {
            return None;
        }
        let cell = TemplateCell(ptr as *const AtomicU64);
        (cell.len() >= W_FILLS).then_some(cell)
    }

    #[inline]
    unsafe fn word(self, i: usize) -> &'static AtomicU64 {
        &*self.0.add(i)
    }

    #[inline]
    unsafe fn len(self) -> usize {
        self.word(W_LEN).load(Ordering::Relaxed) as usize
    }

    #[inline]
    unsafe fn get(self, i: usize) -> u64 {
        self.word(i).load(Ordering::Relaxed)
    }

    #[inline]
    unsafe fn set(self, i: usize, v: u64) {
        self.word(i).store(v, Ordering::Relaxed)
    }

    /// Does this thread own the cell's ShapeIds? Every word but the owner is
    /// read and written only by its owner thread.
    #[inline]
    unsafe fn owned(self) -> bool {
        self.word(W_OWNER).load(Ordering::Acquire) == thread_token()
    }

    /// Claim the cell for this thread, or report whether this thread already
    /// owns it.
    unsafe fn claim(self) -> bool {
        let token = thread_token();
        match self
            .word(W_OWNER)
            .compare_exchange(0, token, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => true,
            Err(owner) => owner == token,
        }
    }

    /// The class fill at `i` (of `n` recorded).
    #[inline]
    unsafe fn class_fill(self, i: usize) -> Fill {
        let (tag, value) = (self.get(W_FILLS + 2 * i), self.get(W_FILLS + 2 * i + 1));
        match tag {
            TAG_NAME => Fill::Name,
            TAG_METHOD => Fill::Method(value as usize),
            TAG_PARENT => Fill::Parent,
            _ => Fill::Bits(value),
        }
    }

    /// Where the prototype fills start: after the class fills.
    #[inline]
    unsafe fn proto_fills_base(self) -> usize {
        W_FILLS + 2 * ((self.get(W_CLASS_FACTS) >> 48) as usize)
    }

    #[inline]
    unsafe fn proto_fill(self, i: usize) -> ProtoFill {
        let base = self.proto_fills_base();
        match self.get(base + 2 * i) {
            TAG_CONSTRUCTOR => ProtoFill::Constructor,
            _ => ProtoFill::Method(self.get(base + 2 * i + 1) as usize),
        }
    }

    /// The recorded class-object template: final shape, fill count and
    /// whether it has a parent, when this thread owns one recorded for
    /// `field_count` and `static_field_mask`.
    #[inline]
    unsafe fn class_template(
        self,
        field_count: u32,
        static_field_mask: u32,
    ) -> Option<(u32, usize, bool)> {
        if !self.owned() {
            return None;
        }
        let shape = self.get(W_CLASS_SHAPE) as u32;
        let facts = self.get(W_CLASS_FACTS);
        (shape != 0
            && facts as u32 == field_count
            && (facts >> 32) as u8 as u32 == static_field_mask)
            .then_some((shape, (facts >> 48) as usize, facts & (1 << 40) != 0))
    }

    /// The recorded prototype template: field count, final shape, fill count
    /// and [[Prototype]] identity, when this thread owns one.
    #[inline]
    unsafe fn proto_template(self) -> Option<(u32, u32, usize, u64)> {
        if !self.owned() {
            return None;
        }
        let shape = self.get(W_PROTO_SHAPE) as u32;
        let facts = self.get(W_PROTO_FACTS);
        (shape != 0).then(|| {
            (
                facts as u32,
                shape,
                (facts >> 32) as usize,
                self.get(W_PROTO_ID),
            )
        })
    }
}

/// Register `cell`, template `class_id`'s template cell, on its vtable entry,
/// so a path that starts from one of its class objects finds it. Codegen emits
/// one call per per-evaluation template at module init.
#[no_mangle]
pub extern "C" fn js_register_class_template_cell(class_id: i64, cell: i64) {
    if class_id <= 0 || class_id > u32::MAX as i64 || cell == 0 {
        return;
    }
    super::super::class_registry::class_set_template_cell(class_id as u32, cell as usize);
}

/// Template `class_id`'s template cell, if codegen registered one.
#[inline]
fn template_cell_of(class_id: u32) -> Option<TemplateCell> {
    let ptr = super::super::class_registry::class_template_cell(class_id)?;
    unsafe { TemplateCell::from_ptr(ptr as *const u64) }
}

/// An own key the runtime adds to a per-evaluation class object after its
/// members: the captured environment (`__perry_ctor_caps`) and the
/// evaluation's prototype object (`#<perry:class-evaluation-prototype>`).
/// Neither is reachable from JS, so adding one never runs user code.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum InternalKey {
    CtorCaps,
    EvaluationPrototype,
}

impl InternalKey {
    pub(crate) fn bytes(self) -> &'static [u8] {
        match self {
            InternalKey::CtorCaps => CTOR_CAPS_KEY,
            InternalKey::EvaluationPrototype => {
                super::class_object_props::CLASS_EVALUATION_PROTOTYPE_KEY
            }
        }
    }
}

#[cfg(test)]
thread_local! {
    /// How many class objects and prototype objects this thread built from a
    /// template's shapes (tests only).
    static TEMPLATE_HITS: std::cell::Cell<(u32, u32)> = const { std::cell::Cell::new((0, 0)) };
}

#[cfg(test)]
fn template_hits() -> (u32, u32) {
    TEMPLATE_HITS.with(std::cell::Cell::get)
}

#[cfg(test)]
fn note_template_hit(prototype: bool) {
    TEMPLATE_HITS.with(|c| {
        let (o, p) = c.get();
        c.set(if prototype { (o, p + 1) } else { (o + 1, p) });
    });
}

/// The own-property key of the evaluation's captured environment.
pub(crate) const CTOR_CAPS_KEY: &[u8] = b"__perry_ctor_caps";

/// A static method's own function object for one evaluation: it runs the
/// declaration's closure-convention entry `code`; its one capture, its home
/// class object, is installed next with [`set_static_method_home`].
pub(super) unsafe fn static_method_value(code: usize) -> *mut crate::closure::ClosureHeader {
    crate::closure::js_closure_alloc(code as *const crate::closure::JsFunctionInfo, 1)
}

/// Give `f`, fresh from [`static_method_value`] with no allocation since,
/// its home class object `home` (NaN-boxed).
pub(super) unsafe fn set_static_method_home(f: *mut crate::closure::ClosureHeader, home: f64) {
    crate::closure::closure_install_boxed_captures(f, &[home.to_bits()]);
}

/// Is `bits` the function object of static method entry `code` whose home is
/// class object `home`?
pub(crate) unsafe fn static_method_value_runs(
    bits: u64,
    code: usize,
    home: *const ObjectHeader,
) -> bool {
    let value = JSValue::from_bits(bits);
    if !value.is_pointer() {
        return false;
    }
    let closure = value.as_pointer::<crate::closure::ClosureHeader>();
    crate::closure::is_closure_ptr(closure as usize)
        && (*closure).info as usize == code
        && crate::closure::js_closure_get_capture_bits(closure, 0)
            == crate::value::js_nanbox_pointer(home as i64).to_bits()
}

/// One evaluation's class object of template `template_class_id`
/// (`ClassExprFresh`): allocated `field_count` slots wide, a class object, with
/// its own `length`, `name` and static methods and its pinned parent.
/// `static_field_mask` (bit 0 `length`, bit 1 `name`) names the static fields
/// that take over an intrinsic key.
/// `cell` is the template's cell (`@perry_ctpl.<cid>`); null takes the
/// ordinary path.
#[no_mangle]
pub extern "C" fn js_class_evaluation_object(
    template_class_id: u32,
    field_count: u32,
    static_field_mask: u32,
    cell: *const u64,
) -> i64 {
    let cell = unsafe { TemplateCell::from_ptr(cell) };
    let template = cell.and_then(|c| unsafe { c.class_template(field_count, static_field_mask) });
    // The template stash, deliberately: this records the heritage the
    // `RegisterClassParentDynamic` call immediately preceding the evaluation
    // evaluated for THIS evaluation. `js_get_dynamic_parent_value`'s
    // active-replay override would answer with an enclosing constructor
    // replay's parent when a factory is re-entered from inside a constructor
    // body. A template recorded without heritage has none to read.
    let has_parent = template.is_none_or(|t| t.2);
    let parent = if has_parent {
        // #11759 (c′): when the template's class function object is the
        // declaration's first evaluation, this is a later one: it pins the
        // parent its own heritage registered, which the template no longer
        // answers with (its class function object keeps the static heritage).
        crate::object::class_value::class_value_is_first_evaluation(template_class_id)
            .then(|| crate::object::class_registry::stashed_dynamic_parent_value(template_class_id))
            .flatten()
            .unwrap_or_else(|| {
                crate::object::parent_static::template_dynamic_parent_value(template_class_id)
            })
    } else {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    };
    if let (Some(cell), Some((final_shape, fills, has_parent))) = (cell, template) {
        if has_parent == (parent.to_bits() != crate::value::TAG_UNDEFINED) {
            return unsafe {
                class_object_in_template_shape(
                    template_class_id,
                    field_count,
                    final_shape,
                    cell,
                    fills,
                    parent,
                )
            } as i64;
        }
    }
    let obj = crate::object::js_object_alloc(template_class_id, field_count);
    crate::object::class_registry::js_object_mark_class(obj as i64);
    unsafe {
        crate::object::parent_static::class_object_define_members(
            obj,
            template_class_id,
            static_field_mask,
            parent,
            &|obj, parent| {
                if let Some(cell) = cell {
                    record_class_object_template(
                        cell,
                        obj,
                        template_class_id,
                        field_count,
                        static_field_mask,
                        parent,
                    )
                }
            },
        ) as i64
    }
}

/// Allocate a class object of template `class_id` directly in the template's
/// `final_shape` and fill its slots.
unsafe fn class_object_in_template_shape(
    class_id: u32,
    field_count: u32,
    final_shape: u32,
    cell: TemplateCell,
    fills: usize,
    parent: f64,
) -> *mut ObjectHeader {
    #[cfg(test)]
    note_template_hit(false);
    let scope = crate::gc::RuntimeHandleScope::new();
    let parent = scope.root_nanbox_f64(parent);
    // The allocation `js_object_alloc(class_id, field_count)` makes, so the
    // object is exactly as wide as the one the shape was recorded on.
    let obj = crate::object::alloc_basic::object_alloc_unpublished(class_id, field_count);
    // Born a class object in its final shape (all `Any` lanes, so every slot
    // holds a valid value while still `undefined`).
    crate::object::shapes::stamp_object_shape_id_with_carrier_note(obj, final_shape);
    // The keys carry non-default attribute entries from birth.
    crate::object::descriptor_state::note_attrs_born_with_keys(obj as usize);
    // `js_object_mark_class`'s bookkeeping for a class object.
    crate::object::field_get_set::note_private_template_evaluated(class_id);
    crate::object::class_registry::class_object_value_root_store(class_id, obj);
    let class = scope.root_raw_mut_ptr(obj);
    for slot in 0..fills {
        let bits = match cell.class_fill(slot) {
            Fill::Bits(bits) => bits,
            Fill::Name => super::super::class_value::intrinsic_own_data_value(class_id, "name")
                .map_or(crate::value::TAG_UNDEFINED, f64::to_bits),
            Fill::Method(code) => {
                let f = static_method_value(code);
                class.with_mut_ptr::<ObjectHeader, _>(|class| {
                    set_static_method_home(f, crate::value::js_nanbox_pointer(class as i64))
                });
                crate::value::js_nanbox_pointer(f as i64).to_bits()
            }
            Fill::Parent => {
                // `js_class_evaluation_object`'s ordering rule: arm before
                // the write it advertises.
                super::super::class_registry::evaluation_heritage::CLASS_OBJECT_HERITAGE_PIN_LATCH
                    .arm();
                parent.get_nanbox_f64().to_bits()
            }
        };
        class.with_mut_ptr::<ObjectHeader, _>(|class| {
            crate::object::slot_store::store_object_field_slot(class, slot, bits)
        });
    }
    class.with_mut_ptr::<ObjectHeader, _>(|class| class)
}

/// After `obj` got its own keys the ordinary way, record the shape it reached
/// as template `class_id`'s, if its keys are exactly the template's, each in
/// its inline slot, and its lanes are all `Any`.
unsafe fn record_class_object_template(
    cell: TemplateCell,
    obj: *mut ObjectHeader,
    class_id: u32,
    field_count: u32,
    static_field_mask: u32,
    parent: f64,
) {
    // The first evaluation on the thread that claims the cell settles it:
    // recorded, or refused for good.
    if !cell.claim() || cell.get(W_CLASS_SETTLED) != 0 {
        return;
    }
    cell.set(W_CLASS_SETTLED, 1);
    let Some(d) = crate::object::shapes::object_shape_descriptor(obj) else {
        return;
    };
    let final_shape = crate::object::shapes::object_shape_id(obj);
    let count = d.logical_key_count as usize;
    if final_shape == 0
        || d.hole_count != 0
        || d.rep != crate::object::field_rep::REP_ANY
        || d.live_inline_slot_count < d.logical_key_count
        || count > (field_count as usize).max(crate::object::INLINE_SLOT_FLOOR)
        || count >= 1 << 16
        || W_FILLS + 2 * count > cell.len()
        || static_field_mask > 0xff
        || crate::object::shapes::shape_object_kind_by_id(final_shape)
            != Some(crate::object::shapes::ShapeObjectKind::Class)
    {
        return;
    }
    let has_parent = parent.to_bits() != crate::value::TAG_UNDEFINED;
    let keys = d.keys_view();
    let statics = super::super::class_registry::class_own_string_member_names(class_id, true);
    let fields = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
    let mut fills = Vec::with_capacity(count);
    let mut saw_parent = false;
    for slot in 0..count {
        let key = keys.get(slot as u32);
        let bits = *fields.add(slot);
        let value = JSValue::from_bits(bits);
        let is = |name: &[u8]| crate::string::js_string_key_matches_bytes(key, name);
        let fill = if is(crate::object::parent_static::CLASS_OBJECT_PARENT_KEY.as_bytes())
            && has_parent
            && bits == parent.to_bits()
        {
            saw_parent = true;
            Fill::Parent
        } else if let Some(name) = statics.iter().find(|name| is(name.as_bytes())) {
            // A static method's own function object (a static accessor of
            // this name leaves an accessor, which no template records).
            let Some(code) =
                super::super::class_registry::class_own_static_method_code(class_id, name)
            else {
                return;
            };
            if !static_method_value_runs(bits, code, obj) {
                return;
            }
            Fill::Method(code)
        } else if (is(b"length") || is(b"name")) && bits == crate::value::TAG_UNDEFINED {
            // The placeholder a static field of this name stores over.
            if static_field_mask == 0 {
                return;
            }
            Fill::Bits(bits)
        } else if is(b"length") && !value.is_pointer() && !value.is_any_string() {
            Fill::Bits(bits)
        } else if is(b"name") && value.is_any_string() {
            Fill::Name
        } else {
            return;
        };
        fills.push(fill);
    }
    if saw_parent != has_parent {
        return;
    }
    // The record stays this agent's for its life, so the template never
    // stamps an id that names other facts.
    crate::object::shapes::note_external_shape_carrier(Some(d));
    for (i, fill) in fills.iter().enumerate() {
        let (tag, value) = match *fill {
            Fill::Bits(bits) => (TAG_BITS, bits),
            Fill::Name => (TAG_NAME, 0),
            Fill::Method(code) => (TAG_METHOD, code as u64),
            Fill::Parent => (TAG_PARENT, 0),
        };
        cell.set(W_FILLS + 2 * i, tag);
        cell.set(W_FILLS + 2 * i + 1, value);
    }
    cell.set(
        W_CLASS_FACTS,
        field_count as u64
            | (static_field_mask as u64) << 32
            | (has_parent as u64) << 40
            | (count as u64) << 48,
    );
    cell.set(W_CLASS_SHAPE, final_shape as u64);
}

/// Add internal own key `key` with `value` to class object `obj`. From a shape
/// the template recorded the addition from, that is one transition: stamp its
/// target, store the slot. Any other object takes the ordinary store, and the
/// template records the transition it made when it appended exactly this key
/// in an inline `Any` slot.
pub(crate) unsafe fn class_object_add_internal(
    obj: *mut ObjectHeader,
    key: InternalKey,
    value: f64,
    cell: Option<TemplateCell>,
) {
    let from = crate::object::shapes::object_shape_id(obj);
    let key_word = key as u64 + 1;
    // The recorded transition from `from` by `key`, and the first free pair
    // when there is none, of a cell this thread owns.
    let edge = cell
        .filter(|c| c.owned() && c.get(W_CLASS_SHAPE) != 0)
        .map(|c| {
            let mut free = None;
            for i in 0..MAX_TEMPLATE_EDGES {
                let (ids, at) = (c.get(W_EDGES + 2 * i), c.get(W_EDGES + 2 * i + 1));
                if at >> 32 == 0 {
                    free.get_or_insert(i);
                } else if ids as u32 == from && at >> 32 == key_word {
                    return (Some(((ids >> 32) as u32, at as u32)), free);
                }
            }
            (None, free)
        });
    if let Some((Some((to, slot)), _)) = edge {
        // `obj` carries the recorded predecessor, so it is exactly as wide as
        // the object the transition was recorded on: `slot` is inline.
        crate::object::shapes::stamp_object_shape_id_with_carrier_note(obj, to);
        crate::object::slot_store::store_object_field_slot(obj, slot as usize, value.to_bits());
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let class = scope.root_raw_mut_ptr(obj);
    let value = scope.root_nanbox_f64(value);
    let bytes = key.bytes();
    let key_str = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    class.with_mut_ptr::<ObjectHeader, _>(|obj| {
        crate::object::js_object_set_field_by_name(obj, key_str, value.get_nanbox_f64())
    });
    let (Some(cell), Some((None, Some(free)))) = (cell, edge) else {
        return;
    };
    if from == 0 {
        return;
    }
    class.with_mut_ptr::<ObjectHeader, _>(|obj| {
        let Some(d) = crate::object::shapes::object_shape_descriptor(obj) else {
            return;
        };
        let to = crate::object::shapes::object_shape_id(obj);
        let Some(slot) = d.logical_key_count.checked_sub(1) else {
            return;
        };
        let fields = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
        if to == 0
            || to == from
            || d.hole_count != 0
            || d.rep != crate::object::field_rep::REP_ANY
            || d.live_inline_slot_count <= slot
            || !crate::string::js_string_key_matches_bytes(d.keys_view().get(slot), bytes)
            || *fields.add(slot as usize) != value.get_nanbox_f64().to_bits()
        {
            return;
        }
        crate::object::shapes::note_external_shape_carrier(
            crate::object::shapes::shape_descriptor_by_id(from),
        );
        crate::object::shapes::note_external_shape_carrier(Some(d));
        cell.set(W_EDGES + 2 * free, from as u64 | (to as u64) << 32);
        cell.set(W_EDGES + 2 * free + 1, slot as u64 | key_word << 32);
    });
}

/// `__perry_ctor_caps` of a per-evaluation class object (`ClassExprFresh`):
/// the captured environment its constructor replays with. `cell` is the
/// template's cell.
#[no_mangle]
pub extern "C" fn js_class_object_set_ctor_caps(obj: i64, caps: f64, cell: *const u64) {
    if obj == 0 {
        return;
    }
    unsafe {
        class_object_add_internal(
            obj as *mut ObjectHeader,
            InternalKey::CtorCaps,
            caps,
            TemplateCell::from_ptr(cell),
        )
    }
}

/// [`class_object_add_internal`] for a class object of template `class_id`,
/// with the template's registered cell.
pub(crate) unsafe fn class_object_add_internal_for(
    obj: *mut ObjectHeader,
    class_id: u32,
    key: InternalKey,
    value: f64,
) {
    class_object_add_internal(obj, key, value, template_cell_of(class_id))
}

/// How one slot of a template's final prototype shape is filled.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ProtoFill {
    /// `constructor`: the evaluation's class object.
    Constructor,
    /// A new function object running this method's closure-convention entry
    /// (its `JsFunctionInfo`), at home in the class object.
    Method(usize),
}

/// The function object one evaluation's prototype holds for method `name` of
/// template `class_id`: it runs the method's closure-convention entry, and its
/// one capture is its home, class object `class`. `None` when the template
/// registered no entry for `name`.
pub(crate) unsafe fn evaluation_method_value(class_id: u32, name: &str, class: f64) -> Option<f64> {
    let code = super::super::class_registry::class_method_entry(class_id, name)?;
    let scope = crate::gc::RuntimeHandleScope::new();
    let class = scope.root_nanbox_f64(class);
    let f = static_method_value(code);
    if f.is_null() {
        return None;
    }
    set_static_method_home(f, class.get_nanbox_f64());
    Some(crate::value::js_nanbox_pointer(f as i64))
}

/// Build the prototype object of class object `class` (template `class_id`)
/// in the template's final prototype shape, linked to `parent_proto`. `None`
/// when the template has no recorded prototype shape or that shape names
/// another [[Prototype]].
pub(crate) unsafe fn prototype_from_template<'s>(
    out: &'s crate::gc::RuntimeHandleScope,
    class: *mut ObjectHeader,
    class_id: u32,
    parent_proto: u64,
) -> Option<crate::gc::RuntimeHandle<'s>> {
    let cell = template_cell_of(class_id)?;
    let (field_count, final_shape, fills, proto_id) = cell.proto_template()?;
    if crate::object::shapes::stable_linked_proto_id(class_id, parent_proto) != Some(proto_id) {
        return None;
    }
    #[cfg(test)]
    note_template_hit(true);
    // Every handle lives in the caller's scope: the prototype handle is
    // returned, and a handle cannot outlive the scope that rooted it.
    let class = out.root_raw_mut_ptr(class);
    let parent = out.root_heap_word_u64(parent_proto);
    let proto = out.root_raw_mut_ptr(crate::object::js_object_alloc(class_id, field_count));
    // The links `class_evaluation_prototype_value` makes, written into the
    // prototype's meta record directly: its evaluation (lexical owner) and its
    // [[Prototype]], whose identity the final shape already names.
    let (meta, _) = proto.across_mut::<ObjectHeader, _>(|| {
        proto.with_mut_ptr::<ObjectHeader, _>(|p| crate::object::object_meta_ensure(p))
    });
    let owner =
        class.with_const_ptr::<ObjectHeader, _>(|c| crate::value::js_nanbox_pointer(c as i64));
    let parent_bits = parent.get_heap_word_u64();
    (*meta).private_evaluation_brand = owner.to_bits();
    crate::gc::runtime_write_barrier_slot(
        meta as usize,
        &(*meta).private_evaluation_brand as *const u64 as usize,
        owner.to_bits(),
    );
    (*meta).prototype = parent_bits;
    crate::gc::runtime_write_barrier_slot(
        meta as usize,
        &(*meta).prototype as *const u64 as usize,
        parent_bits,
    );
    proto.with_mut_ptr::<ObjectHeader, _>(|proto| {
        crate::object::shapes::stamp_object_shape_id_with_carrier_note(proto, final_shape);
        crate::object::descriptor_state::note_attrs_born_with_keys(proto as usize);
    });
    for slot in 0..fills {
        let bits = match cell.proto_fill(slot) {
            ProtoFill::Constructor => class
                .with_const_ptr::<ObjectHeader, _>(|c| crate::value::js_nanbox_pointer(c as i64)),
            ProtoFill::Method(code) => {
                let f = static_method_value(code);
                class.with_mut_ptr::<ObjectHeader, _>(|c| {
                    set_static_method_home(f, crate::value::js_nanbox_pointer(c as i64))
                });
                crate::value::js_nanbox_pointer(f as i64)
            }
        };
        proto.with_mut_ptr::<ObjectHeader, _>(|proto| {
            crate::object::slot_store::store_object_field_slot(proto, slot, bits.to_bits())
        });
    }
    Some(proto)
}

/// After `proto`, the prototype object of class object `class` (template
/// `class_id`), was built the ordinary way and linked to `parent_proto`, record
/// its shape as the template's, if its keys are exactly `constructor` and the
/// template's methods (each an entry-backed function object at home in
/// `class`), each in its inline slot, with all lanes `Any`.
pub(crate) unsafe fn record_prototype_template(
    proto: *mut ObjectHeader,
    class: *const ObjectHeader,
    class_id: u32,
    field_count: u32,
    parent_proto: u64,
) {
    let Some(cell) = template_cell_of(class_id) else {
        return;
    };
    // Only on top of this thread's class-object template: the prototype fills
    // follow its fills in the cell.
    if !cell.owned() || cell.get(W_CLASS_SETTLED) == 0 || cell.get(W_PROTO_SHAPE) != 0 {
        return;
    }
    let Some(proto_id) = crate::object::shapes::stable_linked_proto_id(class_id, parent_proto)
    else {
        return;
    };
    let Some(d) = crate::object::shapes::object_shape_descriptor(proto) else {
        return;
    };
    let final_shape = crate::object::shapes::object_shape_id(proto);
    let count = d.logical_key_count as usize;
    let meta = (*proto).meta;
    let owner = crate::value::js_nanbox_pointer(class as i64).to_bits();
    if final_shape == 0
        || d.proto_id != proto_id
        || d.hole_count != 0
        || d.rep != crate::object::field_rep::REP_ANY
        || d.live_inline_slot_count < d.logical_key_count
        || count > (field_count as usize).max(crate::object::INLINE_SLOT_FLOOR)
        || meta.is_null()
        || (*meta).prototype != parent_proto
        || (*meta).private_evaluation_brand != owner
        || (*meta).flags != 0
        || (*meta).spill != 0
        || u32::try_from(count).is_err()
        || cell.proto_fills_base() + 2 * count > cell.len()
    {
        return;
    }
    let keys = d.keys_view();
    let fields = (proto as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
    let members = super::super::class_registry::class_prototype_member_names(class_id);
    let mut fills = Vec::with_capacity(count);
    for slot in 0..count {
        let key = keys.get(slot as u32);
        let bits = *fields.add(slot);
        let is = |name: &[u8]| crate::string::js_string_key_matches_bytes(key, name);
        let fill = if slot == 0 && is(b"constructor") && bits == owner {
            ProtoFill::Constructor
        } else if let Some((name, false)) = members.iter().find(|(name, _)| is(name.as_bytes())) {
            let Some(code) = super::super::class_registry::class_method_entry(class_id, name)
            else {
                return;
            };
            if !static_method_value_runs(bits, code, class) {
                return;
            }
            ProtoFill::Method(code)
        } else {
            return;
        };
        fills.push(fill);
    }
    crate::object::shapes::note_external_shape_carrier(Some(d));
    let base = cell.proto_fills_base();
    for (i, fill) in fills.iter().enumerate() {
        let (tag, value) = match *fill {
            ProtoFill::Constructor => (TAG_CONSTRUCTOR, 0),
            ProtoFill::Method(code) => (TAG_METHOD, code as u64),
        };
        cell.set(base + 2 * i, tag);
        cell.set(base + 2 * i + 1, value);
    }
    cell.set(W_PROTO_FACTS, field_count as u64 | (count as u64) << 32);
    cell.set(W_PROTO_ID, proto_id);
    cell.set(W_PROTO_SHAPE, final_shape as u64);
}

/// Is `obj` an evaluation prototype of template `class_id` built with the
/// template's method entries, i.e. the sole owner of its declared methods?
pub(crate) unsafe fn is_evaluation_prototype_with_methods(
    obj: *const ObjectHeader,
    class_id: u32,
) -> bool {
    super::class_object_props::class_evaluation_prototype_class_id(obj as usize) == Some(class_id)
        && template_has_method_entries(class_id)
}

/// Does template `class_id` give its evaluations' prototypes their own method
/// function objects (`<method>__eclo`)?
fn template_has_method_entries(class_id: u32) -> bool {
    super::super::class_registry::class_prototype_member_names(class_id)
        .iter()
        .any(|(name, accessor)| {
            !accessor && super::super::class_registry::class_method_entry(class_id, name).is_some()
        })
}

/// For an own-key miss on `obj` whose recorded chain was walked and does not
/// carry `key`: was `key` a method of `obj`'s template that the chain's
/// evaluation prototype no longer owns? Only an object linked through a class
/// evaluation (an evaluation prototype, or an instance of one evaluation) can
/// answer yes, and only when that prototype lost a member: while it still
/// carries every key of the template's final prototype shape, every method the
/// template declares is on the chain, so the miss is not one of them.
/// Otherwise the template's method entries decide.
pub(crate) unsafe fn evaluation_chain_lost_method(
    obj: *const ObjectHeader,
    key: *const crate::string::StringHeader,
) -> bool {
    // Linked through a class evaluation: its shape names a prototype other
    // than the one its template class implies.
    if !crate::object::prototype_chain::object_has_individual_class_prototype(obj as usize) {
        return false;
    }
    let class_id = (*obj).class_id;
    let template = template_cell_of(class_id)
        .and_then(|c| c.proto_template())
        .and_then(|t| crate::object::shapes::shape_descriptor_by_id(t.1));
    if let Some(template) = template {
        // The template's prototype keys, all still there: marking the
        // prototype (its first instance) restamps its shape but keeps them.
        let intact = |o: *const ObjectHeader| {
            crate::object::shapes::object_shape_descriptor(o).is_some_and(|d| {
                d.keys == template.keys
                    && d.logical_key_count == template.logical_key_count
                    && d.hole_count == 0
            })
        };
        if intact(obj) {
            return false;
        }
        let proto = JSValue::from_bits(crate::object::shapes::object_prototype_word(obj));
        if proto.is_pointer() {
            let proto = proto.as_pointer::<ObjectHeader>();
            if crate::value::addr_class::try_read_gc_header(proto as usize)
                .is_some_and(|h| h.obj_type == crate::gc::GC_TYPE_OBJECT)
                && (*proto).class_id == class_id
                && intact(proto)
            {
                return false;
            }
        }
    }
    let bytes = std::slice::from_raw_parts(
        (key as *const u8).add(std::mem::size_of::<crate::StringHeader>()),
        (*key).byte_len as usize,
    );
    let Ok(name) = std::str::from_utf8(bytes) else {
        return false;
    };
    let mut cid = class_id;
    for _ in 0..32 {
        if super::super::class_registry::class_method_entry(cid, name).is_some() {
            return true;
        }
        match super::super::class_registry::get_parent_class_id(cid) {
            Some(parent) if parent != 0 && parent != cid => cid = parent,
            _ => return false,
        }
    }
    false
}

#[cfg(test)]
#[path = "class_object_template_tests.rs"]
mod tests;
