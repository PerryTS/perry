use super::*;
use crate::object::class_image::{
    ImageTable, StaticAccessorTable, StaticMethodTable, StringMemberOrderTable,
};
use std::collections::HashMap;
use std::sync::RwLock;

/// The function objects a class keeps for its declared methods, one per
/// (kind, name), so every path that reads a declared method sees the same
/// object. Each is a traced internal slot of the class's holder, its
/// function object (`class_value_ptr`): never a JS property, never a side
/// table, and it lives exactly as long as the holder.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) enum ClassDeclarationValueKind {
    /// A public string-keyed method of `C.prototype`.
    Method,
    /// A private `#m` or symbol-aliased method: never a prototype property.
    NonPropertyMethod,
    /// A private static method's value of a declared class.
    PrivateStaticMethod,
}

impl ClassDeclarationValueKind {
    /// The fixed head of this kind's holder-internal keys.
    const fn key_head(self) -> &'static [u8] {
        match self {
            Self::Method => b"\x01m:",
            Self::NonPropertyMethod => b"\x01n:",
            Self::PrivateStaticMethod => b"\x01s:",
        }
    }
}

/// The holder-internal key of declaration value (`kind`, `name`):
/// `"\x01" + tag + ":" + name`.
///
/// The holder's internal namespace has other families: private statics
/// (`#x`), runtime-internal keys (`__perry_*`, `#<perry:...>`) and the
/// computed-member names of `declarations.rs` (`"\x01k:"`, `"\x01r"`). None
/// of the others starts with this kind's head, and the fixed three-byte head
/// makes (kind, name) -> key injective, so no member name (a computed
/// `["#x"]()`, or one spelled like another key) can reach another entry.
pub(crate) fn class_declaration_value_key(kind: ClassDeclarationValueKind, name: &str) -> String {
    let head = kind.key_head();
    let mut key = String::with_capacity(head.len() + name.len());
    // SAFETY: the heads are ASCII.
    key.push_str(unsafe { std::str::from_utf8_unchecked(head) });
    key.push_str(name);
    key
}

crate::perry_thread_local! {
    /// Backing LLVM globals for declared static fields, keyed exactly like
    /// `CLASS_DYNAMIC_PROPS`. Direct compiled reads use these cells, while
    /// computed/member writes reach the runtime side table. Remembering the
    /// stable global address lets the runtime keep both views coherent.
    pub(super) static CLASS_DECLARED_STATIC_GLOBAL_SLOTS: std::cell::RefCell<HashMap<u32, HashMap<String, usize>>> =
        std::cell::RefCell::new(HashMap::new());
    /// Exact inverse membership index for `CLASS_PROTOTYPE_OBJECTS`.
    ///
    /// Several class ids may deliberately share one prototype object, so the
    /// value is a reference count rather than a set bit. Keeping this beside
    /// the forward map turns the hot "is this any class's prototype?" probe
    /// from a linear value scan into one pointer-hash lookup. The former
    /// monotone address filter rejected small graphs cheaply but saturated on
    /// class-heavy module graphs and fell back to O(classes).
    pub(super) static CLASS_PROTOTYPE_ADDR_COUNTS: std::cell::RefCell<crate::fast_hash::PtrHashMap<usize, u32>> =
        std::cell::RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

pub(crate) fn is_non_constructable_builtin_function_value(value: f64) -> bool {
    super::super::native_module::builtin_closure_is_non_constructable_value(value)
}

/// True when `value` is a bound native-module *constructor* export. Native
/// constructors and ordinary module functions share `BOUND_METHOD_FUNC_PTR`,
/// so the export's explicit constructor metadata must make the distinction.
pub(crate) fn is_bound_native_constructor_closure_value(value: f64) -> bool {
    super::super::native_module::bound_native_callable_is_constructor_value(value)
}

pub(crate) fn throw_non_constructable_builtin_function() -> ! {
    super::super::object_ops::throw_object_type_error(b"Function is not a constructor")
}

/// Has `delete` removed class `class_id`'s own ClassBody prototype member
/// `name` (a method, an accessor, or `constructor`)? Derived from the object
/// that owns the member: it was declared, the class's decl prototype exists,
/// and that object no longer holds the key.
pub(crate) fn class_proto_key_deleted(class_id: u32, name: &str) -> bool {
    if class_id == 0 {
        return false;
    }
    let proto = class_decl_prototype_object(class_id);
    if proto.is_null() {
        // A lazy holder cannot have lost any of its declaration's keys.
        return false;
    }
    // The complete birth P proves every declared key is still present. This
    // replaces the old latch-byte shortcut without probing the method name.
    if unsafe { crate::object::shapes::pristine_declaration_holder(proto) } {
        return false;
    }
    if proto_member_has_no_string_key(class_id, name) {
        return false;
    }
    // SAFETY: `proto` is this realm's live decl prototype; nothing below
    // allocates.
    !unsafe {
        let keys = crate::object::object_keys(proto);
        let arr = keys.arr();
        !arr.is_null()
            && crate::object::keys_find_slot_by_bytes_resolved(arr, keys.count(), name.as_bytes())
                .is_some()
    }
}

/// A declared prototype member that the reflective prototype object never
/// carries under the string key `name`, so the absence of that key proves
/// nothing about a `delete` (#11692): a `#private` method (not a property at
/// all; `delete` cannot reach it) and the synthetic dispatch alias of a
/// well-known-symbol method (`@@iterator` stands in for `[Symbol.iterator]`,
/// which lives under its symbol key). A source method literally named
/// `"@@iterator"` has a string-member order registration and is a real key.
pub(crate) fn proto_member_has_no_string_key(class_id: u32, name: &str) -> bool {
    if name.starts_with('#') {
        return true;
    }
    internal_symbol_dispatch_alias(name)
        && !CLASS_STRING_MEMBER_ORDERS
            .read()
            .ok()
            .and_then(|guard| {
                guard
                    .as_ref()
                    .map(|map| map.contains_key(&(class_id, false, name.to_string())))
            })
            .unwrap_or(false)
}

/// Has `delete` removed class `class_id`'s own static member `name` (a
/// ClassBody static method or accessor, or the intrinsic `name` / `length`)?
/// Derived from the class function object that owns it.
pub(crate) fn class_static_key_deleted(class_id: u32, name: &str) -> bool {
    crate::object::class_value::class_static_key_deleted(class_id, name)
}

/// Record `C.<name> = value` in the class-ref side table that dynamic reads
/// (`const K: any = C; K.name`, `Object.keys(C)`, `getOwnPropertyDescriptor`)
/// consult, and shade the stored value for the incremental marker.
///
/// Takes `&str`, not `String`: every caller had a value it did not own, so the
/// old signature forced an allocation on EVERY store — and then threw it away,
/// because `HashMap::insert` keeps the original key when one is already
/// present. That is the shape of a `static` counter: codegen emits this call
/// after each `Expr::StaticFieldSet`, so `Shape.made = Shape.made + 1` inside a
/// constructor runs it once per construction (144,000 times in
/// gc-handoff/apps/shapes.ts) and the key exists after the first.
///
/// A static store touches only the class function object: the prototype
/// side lives on the prototype object, so `C.m = 1` can never resurrect a
/// deleted `C.prototype.m`.
pub(crate) fn class_dynamic_prop_root_store(class_id: u32, name: &str, value: f64) {
    // The class function object's own-property bag (barriered, traced).
    crate::object::class_value::class_static_set(class_id, name, value);
    class_static_alias_sync(class_id, name);
}

/// Keep a declared static field's compiled alias (its `@perry_static_*`
/// global, which statically lowered `C.x` reads and writes) coherent with the
/// class function object's own property: the global holds the value while
/// `name` is a plain writable own data property, and `TAG_HOLE` otherwise —
/// deleted, an accessor, or read-only — which sends compiled reads and writes
/// to the generic [[Get]] / [[Set]] (`js_class_static_field_get` / `_put`).
/// Called after every mutation of a class static.
pub(crate) fn class_static_alias_sync(class_id: u32, name: &str) {
    let Some(slot) = CLASS_DECLARED_STATIC_GLOBAL_SLOTS.with(|slots| {
        slots
            .borrow()
            .get(&class_id)
            .and_then(|f| f.get(name))
            .copied()
    }) else {
        return;
    };
    let plain = class_static_defined_attrs(class_id, name).is_none_or(|(writable, _, _)| writable)
        && !crate::object::class_value::class_static_has_own_accessor(class_id, name);
    let value = plain
        .then(|| crate::object::class_value::class_static_get(class_id, name))
        .flatten()
        .unwrap_or(f64::from_bits(crate::value::TAG_HOLE));
    // SAFETY: codegen only registers addresses of process-lifetime LLVM
    // globals, and those slots are mutable GC roots.
    unsafe {
        crate::gc::runtime_store_root_nanbox_f64_raw_slot(slot as *mut f64, value);
    }
}

/// Associate a declared static field's runtime-table entry with the LLVM
/// global used by statically lowered reads and writes. The global has process
/// lifetime and is separately registered with the collector as a mutable root.
pub(crate) fn class_register_declared_static_global_slot(
    class_id: u32,
    name: &str,
    slot: *mut f64,
) {
    if class_id == 0 || name.is_empty() || slot.is_null() {
        return;
    }
    CLASS_DECLARED_STATIC_GLOBAL_SLOTS.with(|slots| {
        let mut slots = slots.borrow_mut();
        let fields = slots.entry(class_id).or_default();
        if let Some(existing) = fields.get_mut(name) {
            *existing = slot as usize;
        } else {
            fields.insert(name.to_string(), slot as usize);
        }
    });
}

/// Store through the class-ref property table and, when this is a declared
/// static, through its compiled backing cell as well. This is the terminal
/// write used by `C.x`, `C["x"]`, and `C[key]` runtime assignment paths.
pub(crate) fn class_ref_dynamic_prop_root_store(class_id: u32, name: &str, value: f64) {
    // The store re-syncs the declared static's compiled alias.
    class_dynamic_prop_root_store(class_id, name, value);
}

/// Own static-field value for a class (no parent-chain walk) — the
/// CLASS_DYNAMIC_PROPS entry codegen registers at module init for every
/// declared static field. Consulted by `getOwnPropertyDescriptor` on a class
/// constructor ref so `verifyProperty(C, "field", …)` sees a real data
/// descriptor (test262 class/elements static-field-declaration & friends).
pub(crate) fn class_own_static_field_value(class_id: u32, name: &str) -> Option<f64> {
    crate::object::class_value::class_static_get(class_id, name)
}

/// Enumerable own string keys of a class constructor: the static fields (and
/// runtime `C.x = …` assignments) recorded in CLASS_DYNAMIC_PROPS. The built-in
/// `length`/`name`/`prototype` slots and static *methods*/*accessors* are
/// non-enumerable, so they are intentionally excluded — this is exactly the set
/// `Object.keys(C)` / `for (k in C)` must yield. Private (`#`) keys are filtered
/// here too (never reflectable). Returned unsorted; the caller applies ECMA
/// ordering. (test262 class/elements static-field-declaration & friends.)
pub(crate) fn class_own_enumerable_field_names(class_id: u32) -> Vec<String> {
    class_own_dynamic_prop_names(class_id)
        .into_iter()
        // #7190: a key installed by `Object.defineProperty` without
        // `enumerable: true` shares this table with static fields but is NOT
        // enumerable.
        .filter(|key| !class_static_key_is_non_enumerable(class_id, key))
        .collect()
}

pub(crate) fn class_own_dynamic_prop_names(class_id: u32) -> Vec<String> {
    crate::object::class_value::class_static_entries(class_id)
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

/// #7190: set the attributes of class `class_id`'s own static data property
/// `name`. They are the key attributes of the class function object's
/// own-property object, as for any ordinary object: a declared `static x = …`
/// field never sets any, so it keeps CreateDataPropertyOrThrow's
/// `(true, true, true)`, and a delete removes them with the key.
pub(crate) fn class_static_set_defined_attrs(
    class_id: u32,
    name: &str,
    writable: bool,
    enumerable: bool,
    configurable: bool,
) {
    {
        let _no_collect = crate::gc::GcSuppressScope::new();
        let ptr = crate::object::class_value::class_value_ptr(class_id) as usize;
        // SAFETY: this agent's live class function object; no collection in
        // this scope.
        let bag = unsafe { crate::closure::props::bag_ensure(ptr) };
        crate::object::set_builtin_property_attrs(
            bag as usize,
            name.to_string(),
            crate::object::PropertyAttrs::new(writable, enumerable, configurable),
        );
    }
    class_static_alias_sync(class_id, name);
}

/// Static `name` becomes an ordinary writable, enumerable, configurable data
/// property again, and its compiled alias is re-synced. A static FIELD
/// definition does this: DefineField creates the property with
/// CreateDataPropertyOrThrow, replacing e.g. the class's own intrinsic `name`.
pub(crate) fn class_static_clear_defined_attrs(class_id: u32, name: &str) {
    let Some(ptr) = crate::object::class_value::class_value_if_minted(class_id) else {
        return;
    };
    // SAFETY: this agent's live class function object.
    let bag = unsafe { crate::closure::props::bag_of(ptr as usize) };
    if bag.is_null() {
        return;
    }
    crate::object::clear_property_attrs(bag as usize, name);
    class_static_alias_sync(class_id, name);
}

/// `(writable, enumerable, configurable)` of class `class_id`'s own static
/// DATA property `name`; `None` when it owns no such data property. Reads
/// the key of the function object's own-property object and never mints the
/// function object (one never created owns no properties).
pub(crate) fn class_static_defined_attrs(class_id: u32, name: &str) -> Option<(bool, bool, bool)> {
    let ptr = crate::object::class_value::class_value_if_minted(class_id)? as usize;
    // SAFETY: this agent's live class function object.
    unsafe {
        if crate::object::is_internal_runtime_key(name)
            || crate::closure::props::bag_get(ptr, name.as_bytes()).is_none()
        {
            return None;
        }
        let bag = crate::closure::props::bag_of(ptr);
        let attrs = crate::object::get_property_attrs(bag as usize, name)
            .unwrap_or(crate::object::PropertyAttrs::new(true, true, true));
        Some((attrs.writable(), attrs.enumerable(), attrs.configurable()))
    }
}

pub(crate) fn class_static_key_is_non_enumerable(class_id: u32, name: &str) -> bool {
    class_static_defined_attrs(class_id, name).is_some_and(|(_, enumerable, _)| !enumerable)
}

/// True when `name` is an own static data property (a static field, or a
/// runtime `C.x = …` assignment) recorded in `CLASS_DYNAMIC_PROPS`. Presence
/// only — does not read the value, so it never invokes a static getter. Used by
/// the `in` operator on a class ref (#6149).
pub(crate) fn class_has_own_dynamic_prop(class_id: u32, name: &str) -> bool {
    crate::object::class_value::class_static_get(class_id, name).is_some()
}

pub(crate) fn class_delete_own_dynamic_prop(class_id: u32, name: &str) {
    crate::object::class_value::class_static_remove(class_id, name);
    class_static_alias_sync(class_id, name);
}

/// The holder-internal key of the array of a class's declared method
/// values, one element per member of its declaration (`declarations.rs`),
/// `undefined` where none was materialized.
pub(crate) const METHOD_VALUES_KEY: &[u8] = b"\x01v";

/// The array of class `class_id`'s kept method values, if its holder has one.
fn method_values(holder: usize) -> Option<*mut crate::ArrayHeader> {
    // SAFETY: this agent's live class function object.
    let value = unsafe {
        crate::closure::props::state_internal_get_joined(holder, METHOD_VALUES_KEY, b"")
    }?;
    let value = crate::JSValue::from_bits(value.to_bits());
    value
        .is_pointer()
        .then(|| value.as_pointer::<crate::ArrayHeader>() as *mut crate::ArrayHeader)
}

/// Does (`kind`, `name`) name a method of the declaration, and is that the
/// kind it is kept as? A name has one kind: `proto_member_has_no_string_key`.
fn declared_method_index(
    class_id: u32,
    kind: ClassDeclarationValueKind,
    name: &str,
) -> Option<(usize, usize)> {
    let non_property = proto_member_has_no_string_key(class_id, name);
    if non_property != (kind == ClassDeclarationValueKind::NonPropertyMethod) {
        return None;
    }
    super::declarations::class_method_decl_index(class_id, name)
}

/// Class `class_id`'s declaration value (`kind`, `name`), if its holder
/// keeps one. Reads only: a class whose holder was never minted has none.
pub(crate) fn class_declaration_value(
    class_id: u32,
    kind: ClassDeclarationValueKind,
    name: &str,
) -> Option<u64> {
    let holder = crate::object::class_value::class_value_if_minted(class_id)? as usize;
    if kind == ClassDeclarationValueKind::PrivateStaticMethod {
        // SAFETY: this agent's live class function object.
        return unsafe {
            crate::closure::props::state_internal_get_joined(
                holder,
                kind.key_head(),
                name.as_bytes(),
            )
        }
        .map(f64::to_bits);
    }
    let (index, _) = declared_method_index(class_id, kind, name)?;
    let values = method_values(holder)?;
    let bits = crate::array::js_array_get_f64(values, index as u32).to_bits();
    (bits != crate::value::TAG_UNDEFINED).then_some(bits)
}

/// Keep `value_bits` as class `class_id`'s declaration value (`kind`,
/// `name`) on its holder. A class whose holder was never minted keeps none:
/// minting builds the class's prototype (its heritage included), which a
/// value read must not cause (a per-evaluation template's evaluation reads
/// its methods without ever building the template's own prototype). Minting
/// itself materializes and keeps every method its prototype holds.
pub(crate) fn class_declaration_value_store(
    class_id: u32,
    kind: ClassDeclarationValueKind,
    name: &str,
    value_bits: u64,
) {
    let Some(holder) = crate::object::class_value::class_value_if_minted(class_id) else {
        return;
    };
    let holder = holder as usize;
    if kind == ClassDeclarationValueKind::PrivateStaticMethod {
        let key = class_declaration_value_key(kind, name);
        // SAFETY: this agent's live, pinned class function object; the
        // internal write allocates under its own GcSuppressScope and stores
        // through the state record's barriered slot.
        unsafe {
            crate::closure::props::state_internal_set(holder, &key, f64::from_bits(value_bits))
        };
        return;
    }
    let Some((index, count)) = declared_method_index(class_id, kind, name) else {
        return;
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_u64(value_bits);
    let values = match method_values(holder) {
        Some(values) => values,
        None => {
            let mut arr = crate::array::js_array_alloc(count as u32);
            let arr_h = scope.root_raw_mut_ptr(arr);
            for _ in 0..count {
                arr = crate::array::js_array_push_f64(
                    arr_h.get_raw_mut_ptr::<crate::ArrayHeader>(),
                    f64::from_bits(crate::value::TAG_UNDEFINED),
                );
                arr_h.set_raw_mut_ptr(arr);
            }
            let arr = arr_h.get_raw_mut_ptr::<crate::ArrayHeader>();
            // SAFETY: as above.
            unsafe {
                crate::closure::props::state_internal_set(
                    holder,
                    // SAFETY: ASCII.
                    std::str::from_utf8_unchecked(METHOD_VALUES_KEY),
                    crate::value::js_nanbox_pointer(arr as i64),
                )
            };
            arr_h.get_raw_mut_ptr::<crate::ArrayHeader>()
        }
    };
    // A declaration registered again with more members (unit tests extend a
    // class member by member) outgrows the array it was kept in.
    let mut values = values;
    if crate::array::js_array_length(values) as usize <= index {
        let arr_h = scope.root_raw_mut_ptr(values);
        while crate::array::js_array_length(arr_h.get_raw_mut_ptr::<crate::ArrayHeader>()) as usize
            <= index
        {
            let grown = crate::array::js_array_push_f64(
                arr_h.get_raw_mut_ptr::<crate::ArrayHeader>(),
                f64::from_bits(crate::value::TAG_UNDEFINED),
            );
            arr_h.set_raw_mut_ptr(grown);
        }
        values = arr_h.get_raw_mut_ptr::<crate::ArrayHeader>();
        // SAFETY: as above.
        unsafe {
            crate::closure::props::state_internal_set(
                holder,
                // SAFETY: ASCII.
                std::str::from_utf8_unchecked(METHOD_VALUES_KEY),
                crate::value::js_nanbox_pointer(values as i64),
            )
        };
    }
    crate::array::js_array_set_f64(values, index as u32, value.get_nanbox_f64());
}

/// Forget the method value kept for member `index` of class `class_id`'s
/// declaration (its computed name changed).
pub(crate) fn class_declaration_value_forget(class_id: u32, index: usize) {
    let Some(holder) = crate::object::class_value::class_value_if_minted(class_id) else {
        return;
    };
    if let Some(values) = method_values(holder as usize) {
        crate::array::js_array_set_f64(
            values,
            index as u32,
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
    }
}

// ============================================================================
// Class member metadata. Instance members are the class's static declaration
// (`declarations.rs`); dispatch reads the prototype holder's data slots.
// ============================================================================

/// The compiled halves of one declared accessor, each 0 when that half is
/// absent: `get` is `fn(this) -> f64`, `set` is `fn(this, value) -> f64`.
/// `set_length` is the setter's spec `.length` (0 for `set m(x = 1)`), when
/// codegen recorded one.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct AccessorDecl {
    pub get: usize,
    pub set: usize,
    pub set_length: Option<u32>,
}

/// #1788: per-class STATIC-method registry: class_id -> { name -> (func_ptr,
/// param_count, has_rest) }. Static methods are emitted as `perry_static_*`
/// (no `this` param — they resolve `this` via `js_static_this_resolve`) and are
/// NOT instance members, so a subclass whose parent is a
/// class-expression value (`class Sub extends make(...) {}`) can't resolve an
/// inherited static method (`Sub.greet()`) at compile time. This table is
/// walked up the class_id parent chain at runtime by
/// `js_class_static_method_call`. `has_rest` marks a trailing rest param
/// (`static pipe(...args)`, effect's `pipe`/`dual`) so the dispatcher bundles
/// the call args into an array for that slot.
pub static CLASS_STATIC_METHODS: ImageTable<RwLock<Option<StaticMethodTable>>> =
    ImageTable::new(|image| &image.static_methods);

/// Static accessors on the class constructor: class_id -> { name ->
/// [`AccessorDecl`] }, each half 0 when absent.
pub static CLASS_STATIC_ACCESSORS: ImageTable<RwLock<Option<StaticAccessorTable>>> =
    ImageTable::new(|image| &image.static_accessors);

/// Source order for public class methods/accessors. Dispatch data lives in
/// separate hash maps by member kind; keeping this metadata alongside the
/// image lets [[OwnPropertyKeys]] reconstruct the single ClassBody order.
pub static CLASS_STRING_MEMBER_ORDERS: ImageTable<RwLock<Option<StringMemberOrderTable>>> =
    ImageTable::new(|image| &image.string_member_orders);

/// Spec `Function.prototype.length` per (class_id, method/accessor name) — the
/// count of formal parameters before the first one with a default or a rest.
/// The vtable only records the *total* param count (needed for call dispatch),
/// which overcounts methods with default-valued params; codegen computes the
/// real `.length` at registration and stashes it here so `C.prototype.m.length`
/// is exact (Test262 .../class/*/dflt-params-trailing-comma).
pub static CLASS_METHOD_BIND_LENGTHS: ImageTable<RwLock<Option<HashMap<(u32, String), u32>>>> =
    ImageTable::new(|image| &image.method_bind_lengths);

/// The closure-convention entry declared for method `name` of class
/// `class_id`, if any.
pub(crate) fn class_method_entry(class_id: u32, name: &str) -> Option<usize> {
    let entry = super::declarations::class_method_decl(class_id, name)?.entry as usize;
    (entry != 0).then_some(entry)
}

/// Default-aware spec `.length` for STATIC methods, keyed (class_id, name).
/// Distinct from `CLASS_METHOD_BIND_LENGTHS` (instance methods) so a class with
/// both `static m(a, b = 1)` and `m(c)` keeps independent lengths instead of
/// colliding on the (class_id, name) key. (Test262 *-method-static
/// dflt-params-trailing-comma.)
pub static CLASS_STATIC_METHOD_BIND_LENGTHS: ImageTable<
    RwLock<Option<HashMap<(u32, String), u32>>>,
> = ImageTable::new(|image| &image.static_method_bind_lengths);

crate::perry_thread_local! {
    pub static CLASS_SYMBOL_METHODS: RwLock<Option<HashMap<(u32, usize, bool), (usize, u32, bool)>>> =
        RwLock::new(None);

    pub static CLASS_SYMBOL_ACCESSORS: RwLock<Option<HashMap<(u32, usize, bool), (usize, usize)>>> =
        RwLock::new(None);

    /// Source order for Symbol-keyed class methods/accessors. Symbol member
    /// registries are thread-local because their keys are heap addresses, so
    /// their ordering metadata follows the same ownership model.
    pub static CLASS_SYMBOL_MEMBER_ORDERS: RwLock<Option<HashMap<(u32, u64, bool), u32>>> =
        RwLock::new(None);
}

/// Set of all registered class ids. Populated at module init by codegen
/// emitting `js_register_class_id(cid)` for every user class — even
/// classes without any methods. Refs #618 / #420 followup.
pub static REGISTERED_CLASS_IDS: ImageTable<RwLock<Option<crate::fast_hash::PtrHashSet<u32>>>> =
    ImageTable::new(|image| &image.registered_class_ids);

crate::perry_thread_local! {
    /// Issue #711 part 2: `function Base() {}; Base.prototype = obj` pattern.
    /// Effect's `internal/effectable.ts` declares classes via prototype
    /// assignment on a plain function, not via `class` syntax. To make
    /// `class Derived extends Base {}` walk into `obj`'s methods at dispatch
    /// time, we model this as a synthetic class:
    ///   - `js_set_function_prototype(func, obj)` allocates a synthetic
    ///     class_id (high-bit-set to avoid collision with codegen-assigned
    ///     ids), stores `func_bits → synthetic_cid` in `FUNCTION_CLASS_IDS`,
    ///     and `synthetic_cid → obj_ptr` in `CLASS_PROTOTYPE_OBJECTS`.
    ///   - `js_register_class_parent_dynamic` extends to detect closure
    ///     parent values, looks up the synthetic class_id, and registers
    ///     the (child, synthetic) edge in CLASS_REGISTRY.
    ///   - The method-dispatch chain walk in `js_native_call_method`
    ///     consults `CLASS_PROTOTYPE_OBJECTS` when it reaches a synthetic
    ///     class_id: it resolves the method as a regular field lookup on
    ///     the prototype object and calls it with `this` bound to the
    ///     receiver.
    pub static FUNCTION_CLASS_IDS: RwLock<Option<HashMap<u64, u32>>> = RwLock::new(None);
}
// Stored as `usize` (raw address) so the map is Send + Sync. The
// pointer is always converted back to `*mut ObjectHeader` at call sites
// (`class_prototype_object` / the dispatch walk) where single-threaded
// usage is guaranteed.
crate::perry_thread_local! {
    pub static CLASS_PROTOTYPE_OBJECTS: RwLock<Option<HashMap<u32, usize>>> = RwLock::new(None);
}

#[inline]
pub(crate) fn class_prototype_object_addr_index_contains(addr: usize) -> bool {
    CLASS_PROTOTYPE_ADDR_COUNTS.with(|index| index.borrow().contains_key(&addr))
}

/// Apply one forward-map value change to the exact inverse membership index.
/// GC root visitors call this after rewriting a prototype address; ordinary
/// stores call it after replacing a class id's prototype.
pub(crate) fn class_prototype_object_addr_index_rekey(old: usize, new: usize) {
    if old == new {
        return;
    }
    CLASS_PROTOTYPE_ADDR_COUNTS.with(|index| {
        let mut index = index.borrow_mut();
        if old != 0 {
            if let Some(count) = index.get_mut(&old) {
                if *count == 1 {
                    index.remove(&old);
                } else {
                    *count -= 1;
                }
            }
        }
        if new != 0 {
            *index.entry(new).or_insert(0) += 1;
        }
    });
}

crate::perry_thread_local! {
    /// #36 / #321: maps a child class_id to the raw address of a parent CLOSURE
    /// (function value) when `class Child extends <function value> {}`. effect's
    /// `class Svc extends Context.Tag("Svc")<...>() {}` extends the function
    /// `TagClass` returned by `Tag(id)()`. In JS this sets `Svc.__proto__ =
    /// TagClass` so static-property reads on `Svc` (`Svc.key`, `Svc._op`,
    /// `Svc[TagTypeId]`) walk to the parent function's own props + ITS static
    /// prototype. Perry's existing dynamic-parent path only models OBJECT parents
    /// (class-expression values), so this records the closure-parent axis so the
    /// class-ref static getters can reach the closure's props and proto chain.
    /// Stored as `usize` (raw address) for Send + Sync; converted back at use.
    pub static CLASS_PARENT_CLOSURES: RwLock<Option<HashMap<u32, usize>>> = RwLock::new(None);
}

crate::perry_thread_local! {
    /// Maps a child class_id to the raw NaN-boxed bits of the parent constructor
    /// VALUE that `js_register_class_parent_dynamic` evaluated at class-definition
    /// time. For `class X extends _mod.default {}` (the interop ESM
    /// default-export-class pattern), the extends expression references a require
    /// alias (`_mod`) that is an IIFE-local — bound only in the module-init scope.
    /// The decl-time registration evaluates it there correctly, so we stash the
    /// resulting value here keyed by the child's class id. `super()` then reads it
    /// back via `js_get_dynamic_parent_value` instead of re-evaluating the extends
    /// expression inside the constructor (where the IIFE-local alias is NOT
    /// captured and the member read would throw "Cannot read properties of
    /// undefined"). Stored as raw `u64` bits, covering both ClassRef (INT32-tagged)
    /// and object/closure (POINTER-tagged) parents.
    pub static CLASS_DYNAMIC_PARENT_VALUE: RwLock<Option<HashMap<u32, u64>>> = RwLock::new(None);
}

/// The `js_register_class_parent_dynamic` stash for `class_id`: the parent
/// value its latest evaluation registered.
pub(crate) fn stashed_dynamic_parent_value(class_id: u32) -> Option<f64> {
    CLASS_DYNAMIC_PARENT_VALUE.with(|table| {
        let guard = table.read().unwrap();
        guard
            .as_ref()
            .and_then(|m| m.get(&class_id).copied())
            .map(f64::from_bits)
    })
}

crate::perry_thread_local! {
    /// #6530: maps a template class_id to the raw NaN-boxed POINTER bits of the
    /// per-evaluation CLASS OBJECT the class statement materialized as (marked by
    /// `js_object_mark_class`). A capture-carrying class has no INT32 ClassRef
    /// value at runtime — the class VALUE is this heap object — so
    /// `instance.constructor` must hand back the same object the module scope /
    /// exports hold, or identity checks (`x.constructor === Sub`, bundled zod's
    /// `describe()` re-construction via `this.constructor`) break. Last-wins
    /// across evaluations of the same class statement, matching the template-cid
    /// compromise used by the sibling tables above.
    pub static CLASS_OBJECT_VALUES: RwLock<Option<HashMap<u32, u64>>> = RwLock::new(None);
}

/// Monotone: has any per-evaluation class object (`ClassExprFresh`) been
/// recorded in this process? While clear, `class_object_value_for_cid` is
/// `None` for every class, so a read of a class function object's own data
/// property answers from its own-property object without consulting the
/// per-evaluation table first.
pub(crate) static CLASS_OBJECT_EVER: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Store the marked class object for its template class id (see
/// `CLASS_OBJECT_VALUES`).
pub(crate) fn class_object_value_root_store(class_id: u32, obj_ptr: *mut ObjectHeader) {
    if class_id == 0 || obj_ptr.is_null() {
        return;
    }
    // #11759 (c′): the class function object is the declaration's first
    // evaluation, and a later evaluation is a class of its own. It never
    // stands in for the template: `C.x` on the first evaluation and the
    // `constructor` of its instances stay the first evaluation's.
    if crate::object::class_value::class_value_is_first_evaluation(class_id) {
        return;
    }
    CLASS_OBJECT_EVER.store(true, std::sync::atomic::Ordering::Relaxed);
    let bits = crate::value::js_nanbox_pointer(obj_ptr as i64).to_bits();
    CLASS_OBJECT_VALUES.with(|table| {
        let mut guard = table.write().unwrap();
        if guard.is_none() {
            *guard = Some(HashMap::new());
        }
        guard.as_mut().unwrap().insert(class_id, bits);
    });
    crate::gc::runtime_write_barrier_root_raw_ptr(obj_ptr);
}

/// Has a class object of template `class_id` been created? The answer is the
/// template's own entry in `CLASS_OBJECT_VALUES`, stored with its first class
/// object, not a copy of it: the answer stays `true` only while no writer
/// removes an entry. A program with no per-evaluation class pays one relaxed
/// load.
#[inline]
pub(crate) fn template_has_class_objects(class_id: u32) -> bool {
    CLASS_OBJECT_EVER.load(std::sync::atomic::Ordering::Relaxed)
        && class_object_value_for_cid(class_id).is_some()
}

/// Read back the class object registered for `class_id`, or `None` when the
/// class never materialized as a per-evaluation object (ordinary
/// ClassRef-valued classes).
pub(crate) fn class_object_value_for_cid(class_id: u32) -> Option<f64> {
    if class_id == 0 {
        return None;
    }
    CLASS_OBJECT_VALUES.with(|table| {
        table.read().ok().and_then(|guard| {
            guard
                .as_ref()
                .and_then(|map| map.get(&class_id).copied())
                .map(f64::from_bits)
        })
    })
}

/// # Any mark that allocates must be the LAST thing its caller does with the
/// pointer
///
/// This function holds `proto_ptr` as a bare pointer and re-uses it AFTER the
/// registry insert, for `class_prototype_object_addr_index_rekey` and for
/// `runtime_write_barrier_root_raw_ptr`. Anything inserted here that can
/// allocate — a mark, a hook, a counter that ensures a side record — can
/// trigger a collection that MOVES the object, and both of those later uses
/// then run on a stale address.
///
/// #10842 learned this by adding one line: marking the registered prototype
/// with `proto_validity::mark_object_as_prototype`, which calls
/// `object_meta_ensure`, SIGSEGV'd the runtime suite. The mark now happens at
/// the `[[Prototype]]` install funnel and, as a self-healing backstop, inside
/// the inherited-read cache's walk, which marks and then immediately abandons
/// the walk precisely so that no pointer it was holding is touched afterwards.
///
/// If you need to add something here that allocates: root `proto_ptr` in a
/// `RuntimeHandleScope` and reload it after, or do the work in the CALLER
/// before it takes the pointer.
pub(crate) fn class_prototype_object_root_store(class_id: u32, proto_ptr: *mut ObjectHeader) {
    if class_id == 0 || proto_ptr.is_null() {
        return;
    }
    let old = CLASS_PROTOTYPE_OBJECTS.with(|table| {
        let mut guard = table.write().unwrap();
        if guard.is_none() {
            *guard = Some(HashMap::new());
        }
        guard.as_mut().unwrap().insert(class_id, proto_ptr as usize)
    });
    if let Some(old) = old.filter(|&old| old != 0 && old != proto_ptr as usize) {
        // SAFETY: the registry held `old` as a live root until this store.
        unsafe {
            super::construct::forget_birth_record_of_class(old as *mut ObjectHeader, class_id)
        };
        retire_displaced_decl_prototype(old as *mut ObjectHeader);
    }
    class_prototype_object_addr_index_rekey(old.unwrap_or(0), proto_ptr as usize);
    crate::gc::runtime_write_barrier_root_raw_ptr(proto_ptr);
    // The effective holder is the CLASS identity's word. A declared
    // holder has precedence over the synthetic input, as on ordinary Get.
    if class_decl_prototype_object(class_id).is_null()
        && ((super::prototype_objects::SYNTHETIC_CLASS_ID_BASE
            ..super::prototype_objects::SYNTHETIC_CLASS_ID_END)
            .contains(&class_id)
            || !super::is_class_object_ptr(proto_ptr as *const u8))
    {
        crate::object::shapes::write_identity_word(
            crate::object::shapes::PROTO_ID_CLASS | u64::from(class_id),
            crate::value::POINTER_TAG | proto_ptr as u64,
        );
    }
}

/// `Object.setPrototypeOf(Ctor, proto)`: the class function object's
/// recorded `[[Prototype]]` (its state record, a traced edge).
pub(crate) fn class_static_prototype_root_store(class_id: u32, proto_ptr: *mut ObjectHeader) {
    if class_id == 0 || proto_ptr.is_null() {
        return;
    }
    let bits = crate::value::js_nanbox_pointer(proto_ptr as i64).to_bits();
    crate::closure::closure_set_static_prototype(
        crate::object::class_value::class_value_ptr(class_id) as usize,
        bits,
    );
}

/// `Object.setPrototypeOf(Ctor, null)`.
pub(crate) fn class_static_prototype_root_clear(class_id: u32) {
    if class_id == 0 {
        return;
    }
    crate::closure::closure_set_static_prototype(
        crate::object::class_value::class_value_ptr(class_id) as usize,
        crate::value::TAG_NULL,
    );
}

fn class_recorded_prototype_bits(class_id: u32) -> Option<u64> {
    // Constructor-chain reads can reach builtin/synthetic parents. Their
    // prototypes are handled by builtin dispatch, not compiled-class closures.
    if class_id == 0 || class_id >= 0x7FFF_FF00 {
        return None;
    }
    let class_id = crate::object::class_generic_origin(class_id).unwrap_or(class_id);
    crate::closure::closure_static_prototype(
        crate::object::class_value::class_value_ptr(class_id) as usize
    )
}

/// True when `Object.setPrototypeOf(Ctor, null)` explicitly severed the
/// constructor's prototype chain, as opposed to never having linked one.
pub(crate) fn class_static_prototype_is_nulled(class_id: u32) -> bool {
    class_recorded_prototype_bits(class_id) == Some(crate::value::TAG_NULL)
}

/// The constructor-side `[[Prototype]]` recorded for `class_id`, or null.
pub(crate) fn class_static_prototype(class_id: u32) -> *mut ObjectHeader {
    match class_recorded_prototype_bits(class_id) {
        Some(bits) if bits & crate::value::TAG_MASK == crate::value::POINTER_TAG => {
            (bits & crate::value::POINTER_MASK) as *mut ObjectHeader
        }
        _ => std::ptr::null_mut(),
    }
}

/// Link `proto_ptr` as declared class `class_id`'s `prototype` object: the
/// class function object's own link (`class_value::class_decl_prototype_link`).
pub(crate) fn class_decl_prototype_object_root_store(class_id: u32, proto_ptr: *mut ObjectHeader) {
    if class_id == 0 || proto_ptr.is_null() {
        return;
    }
    // A declared prototype uses the same existing prototype mark as a chain
    // hop. The mark gives it a private shape lineage, so a property-store PIC
    // cannot learn on another object and later bypass this holder's first
    // ordinary mutation (and current main's direct-call guard retirement).
    // Marking may collect: root and reload before publishing the class link.
    let scope = crate::gc::RuntimeHandleScope::new();
    let prototype = scope.root_raw_mut_ptr(proto_ptr);
    prototype.with_mut_ptr::<ObjectHeader, _>(|proto| unsafe {
        crate::object::proto_validity::mark_object_as_prototype(proto as usize)
    });
    let proto_ptr = prototype.get_raw_mut_ptr::<ObjectHeader>();
    link_decl_prototype_object(class_id, proto_ptr);
}

/// Publish the birth's rooted holder before installing its members: building
/// a method value can ask for the class link again. No JavaScript observes the
/// partial birth. The caller marks its completed shape before returning it.
fn link_decl_prototype_object(class_id: u32, proto_ptr: *mut ObjectHeader) {
    let displaced =
        crate::object::class_value::class_decl_prototype_link_store(class_id, proto_ptr);
    crate::object::shapes::write_identity_word(
        crate::object::shapes::PROTO_ID_CLASS | u64::from(class_id),
        crate::value::POINTER_TAG | proto_ptr as u64,
    );
    if !displaced.is_null() && displaced != proto_ptr {
        retire_displaced_decl_prototype(displaced);
    }
}

/// A bare CLASS prototype identity (`shapes::PROTO_ID_CLASS | class`) names
/// its holder through the class's prototype link, so the link `class -> C.prototype` is a
/// fact of every receiver ShapeId that carries the identity. The link is
/// written once per class identity; a write that REPLACES it (or a
/// generic-origin redirect that changes what it answers) must leave no site
/// trusting the old holder. The displaced prototype takes a semantic shape
/// transition: a process-unique ShapeId no site was trained on. Every site
/// that names that holder compares its ShapeId on each hit, so the relink is
/// seen through shapes alone, without a global generation word.
pub(crate) fn retire_displaced_decl_prototype(old: *mut ObjectHeader) {
    if old.is_null() {
        return;
    }
    // The mint is a no-move window here: `old` is a raw link address.
    let _no_move = crate::gc::GcSuppressScope::new();
    // SAFETY: `old` was a linked (rooted) prototype object until the store
    // above, and nothing between that read and here can collect.
    unsafe {
        if crate::object::shapes::object_shape_stamp(old) != 0 {
            crate::object::shapes::transition_object_shape_semantics(old);
        }
    }
}

pub(crate) fn class_parent_closure_root_store(class_id: u32, closure_addr: usize) {
    if class_id == 0 || closure_addr == 0 {
        return;
    }
    CLASS_PARENT_CLOSURES.with(|table| {
        let mut guard = table.write().unwrap();
        if guard.is_none() {
            *guard = Some(HashMap::new());
        }
        guard.as_mut().unwrap().insert(class_id, closure_addr);
    });
    crate::gc::runtime_write_barrier_root_raw_ptr(closure_addr as *const u8);
}

/// Look up the parent-closure address recorded for a child class_id, if any.
pub(crate) fn class_parent_closure(class_id: u32) -> Option<usize> {
    CLASS_PARENT_CLOSURES.with(|table| {
        table
            .read()
            .ok()
            .and_then(|g| g.as_ref().and_then(|m| m.get(&class_id).copied()))
    })
}

/// Walk the class parent chain looking for a registered parent-closure edge.
/// `super()` dispatch needs this because the instance's class_id is the
/// MOST-DERIVED class, while the closure-parent edge is keyed by the class
/// that directly `extends <function value>` — possibly an ancestor.
pub(crate) fn parent_closure_in_chain(class_id: u32) -> Option<usize> {
    let mut cid = class_id;
    let mut depth = 0u32;
    while depth < 32 && cid != 0 {
        if let Some(addr) = class_parent_closure(cid) {
            return Some(addr);
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    None
}

/// Walk the class parent chain for the nearest ancestor that `extends` a
/// global built-in constructor (`class X extends Array`, or `class Y extends X`
/// above it) and return that built-in's constructor value. A built-in parent
/// registers only its reserved class id as the chain edge, never a
/// parent-closure edge, so its static surface (`Array[Symbol.species]`) is
/// reached through the parent value `js_register_class_parent_dynamic` stashed
/// at definition time (#11193).
pub(crate) fn builtin_parent_ctor_in_chain(class_id: u32) -> Option<f64> {
    let mut cid = class_id;
    let mut depth = 0u32;
    while depth < 32 && cid != 0 {
        let parent = super::parent_static::template_dynamic_parent_value(cid);
        if identify_global_builtin_constructor(parent).is_some() {
            return Some(parent);
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    None
}

/// Reverse lookup: which declared class's `.prototype` is this heap object?
/// Used by `Object.getOwnPropertyDescriptor(C.prototype, name)` to surface
/// vtable accessors as own properties of the prototype object.
///
/// Callers ask about arbitrary objects (#9180: on a bundled application most
/// asks are misses, run thousands of times during module init), so the answer
/// is two reads, never a walk: a declared prototype is allocated with its
/// class's identity id in its header (`class_decl_prototype_value`), a word
/// that is never rewritten after birth, and it is exactly the object that
/// class's link names. Any other object either carries another class id or
/// is not the linked one.
pub(crate) fn class_id_for_decl_prototype_object(ptr: usize) -> Option<u32> {
    decl_prototype_class_id(ptr)
        // #11043: a capture-carrying class (`ClassExprFresh`) gets a distinct
        // prototype object per evaluation instead of the link above, but it
        // is built exactly like one — physical constructor + methods,
        // ClassBody accessors living only in the template's vtable. Every
        // reflection site keyed on this lookup (descriptors, own keys,
        // `defineProperty`, `hasOwn`, `delete`) must therefore see it too, or
        // its accessors are invisible and `Object.defineProperties(C.prototype,
        // { x: { enumerable: true } })` replaces `get x`/`set x` with a
        // read-only `undefined` data property (whatwg-url's `URL`).
        .or_else(|| super::super::field_get_set::class_evaluation_prototype_class_id(ptr))
}

/// The declared class whose linked prototype is the object at `ptr`.
///
/// The answer is proven by the final compare, not by the header read: the
/// link names exactly one live object start, so whatever class id a header
/// read at a non-prototype yields, that class's link is not `ptr`. The read
/// itself only needs to be safe for a live allocation or non-pointer bits
/// (`try_read_gc_header`'s contract), which is what every caller passes.
#[inline]
fn decl_prototype_class_id(ptr: usize) -> Option<u32> {
    // SAFETY: a live GC allocation's address or arbitrary non-pointer bits.
    let header = unsafe { crate::value::addr_class::try_read_gc_header(ptr) }?;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    // SAFETY: the header names an ordinary object at `ptr`; its first word
    // is the class id.
    let class_id = unsafe { (*(ptr as *const ObjectHeader)).class_id };
    (class_id != 0
        && crate::object::class_value::class_decl_prototype_link(class_id) as usize == ptr)
        .then_some(class_id)
}

/// #7757: a monomorphized specialization (`Gen$num`) must present the GENERIC's
/// reflective surface. TypeScript erases type arguments, so at runtime there is
/// exactly one `Gen`, one `Gen.prototype` and one `Gen.prototype.constructor` —
/// the specializations are an implementation detail of monomorphization
/// (`monomorph::mangle::generate_specialized_name`), and without this redirect
/// each one materialized its OWN decl-prototype object. That made
/// `a.constructor !== Gen`, `a.constructor !== b.constructor` and
/// `getPrototypeOf(a) !== getPrototypeOf(b)` where node says all three are
/// equal.
///
/// Same origin edge `instanceof` uses (#7575) and the display name uses
/// (#7632), applied to the third and last identity surface. Method dispatch
/// uses the prototype holder named by the specialization's shape.
pub(crate) fn decl_prototype_identity_id(class_id: u32) -> u32 {
    crate::object::class_generic_origin(class_id).unwrap_or(class_id)
}

/// `C.prototype` of declared class `class_id` (a specialization answers with
/// its generic's) if this agent built it, else null. The class function
/// object's link: indexed loads, no lock, no map.
#[inline]
pub(crate) fn class_decl_prototype_object(class_id: u32) -> *mut ObjectHeader {
    crate::object::class_value::class_decl_prototype_link(decl_prototype_identity_id(class_id))
}

fn internal_symbol_dispatch_alias(name: &str) -> bool {
    matches!(
        name,
        "@@iterator"
            | "@@asyncIterator"
            | "@@toPrimitive"
            | "__perry_dispose__"
            | "__perry_async_dispose__"
            | "__perry_inspect_custom__"
    )
}

fn order_class_string_member_names(class_id: u32, is_static: bool, names: &mut Vec<String>) {
    names.sort();
    names.dedup();
    let orders = CLASS_STRING_MEMBER_ORDERS.read().ok();
    let order_for = |name: &str| {
        orders
            .as_ref()
            .and_then(|guard| guard.as_ref())
            .and_then(|map| map.get(&(class_id, is_static, name.to_string())).copied())
    };
    // Synthetic names exist solely to keep Perry's string-based dispatch fast.
    // A source method literally named "@@iterator" has an order registration
    // and remains visible; an alias created for a Symbol method does not.
    names.retain(|name| {
        let order = order_for(name);
        order != Some(u32::MAX) && (order.is_some() || !internal_symbol_dispatch_alias(name))
    });
    crate::cold_sort::sort_by(names, |left, right| {
        match (order_for(left), order_for(right)) {
            (Some(a), Some(b)) => a.cmp(&b).then_with(|| left.cmp(right)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => left.cmp(right),
        }
    });
}

/// Own string-keyed methods and accessors in ClassBody definition order.
/// The dispatch registries intentionally remain hash maps; reflection is the
/// only consumer that needs their cross-kind ordering.
pub(crate) fn class_own_string_member_names(class_id: u32, is_static: bool) -> Vec<String> {
    let mut names = Vec::new();
    if is_static {
        if let Ok(methods) = CLASS_STATIC_METHODS.read() {
            if let Some(map) = methods.as_ref().and_then(|all| all.get(&class_id)) {
                names.extend(map.keys().cloned());
            }
        }
        if let Ok(accessors) = CLASS_STATIC_ACCESSORS.read() {
            if let Some(map) = accessors.as_ref().and_then(|all| all.get(&class_id)) {
                names.extend(map.keys().cloned());
            }
        }
    } else {
        let _no_move = crate::gc::GcSuppressScope::new();
        let holder = class_holder_prototype(class_id);
        let holder = if holder.is_null() {
            let value = crate::JSValue::from_bits(class_decl_prototype_value(class_id).to_bits());
            if !value.is_pointer() {
                return names;
            }
            value.as_pointer::<ObjectHeader>() as *mut ObjectHeader
        } else {
            holder
        };
        // Reflect the current holder's keys, including deletion/recreation
        // order, rather than reconstructing them from declaration input.
        unsafe {
            let keys = crate::object::object_keys(holder);
            let mut short = [0; crate::value::SHORT_STRING_MAX_LEN];
            for i in 0..keys.count() {
                let value = crate::array::js_array_get(keys.arr(), i);
                if let Some(bytes) = crate::string::js_string_key_bytes(value, &mut short) {
                    if let Ok(name) = std::str::from_utf8(bytes) {
                        names.push(name.to_string());
                    }
                }
            }
        }
        return names;
    }
    names.retain(|name| !name.starts_with('#'));
    order_class_string_member_names(class_id, is_static, &mut names);
    names
}

pub(super) fn install_class_decl_prototype_method_field(
    proto: *mut ObjectHeader,
    class_id: u32,
    name: &str,
) {
    if proto.is_null() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto_handle = scope.root_raw_mut_ptr(proto);
    // Do not bind by reading the prototype object here. Its implicit
    // `constructor` data property would shadow a computed method with that
    // name and make the installation write the class constructor straight
    // back. The canonical vtable value is the property value we need.
    let method_handle =
        scope.root_nanbox_f64(class_prototype_method_value_for_name(class_id, name));
    let key_handle = scope.root_string_ptr(crate::string::js_string_from_bytes(
        name.as_ptr(),
        name.len() as u32,
    ));
    let method = method_handle.get_nanbox_f64();
    proto_handle.with_mut_ptr::<ObjectHeader, _>(|proto| {
        key_handle.with_const_ptr::<crate::StringHeader, _>(|key| {
            // A ClassBody member defines an own property. It must not run an
            // inherited setter while this prototype is being initialized.
            unsafe {
                crate::object::object_ops::define_property_force_store_value(proto, key, method)
            }
        })
    });
    proto_handle.with_mut_ptr::<ObjectHeader, _>(|proto| {
        set_builtin_property_attrs(
            proto as usize,
            name.to_string(),
            PropertyAttrs::new(true, false, true),
        )
    });
}

/// Install one computed Symbol ClassBody member on the materialized declared
/// prototype. The class registries retain compiled dispatch metadata, while
/// the prototype shape is the authoritative record that the property exists:
/// deleting the shape entry therefore cannot be resurrected by a registry
/// fallback.
pub(super) fn install_class_decl_prototype_symbol_member(
    proto: *mut ObjectHeader,
    class_id: u32,
    sym_key: usize,
) {
    if proto.is_null() || sym_key == 0 {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto_h = scope.root_raw_mut_ptr(proto);
    let sym = f64::from_bits(crate::JSValue::pointer(sym_key as *const u8).bits());
    let sym_h = scope.root_nanbox_f64(sym);
    // SAFETY: `sym_key` came from the registered ClassBody symbol table and
    // remains rooted by that registry for the class lifetime.
    let display_name = unsafe { crate::symbol::symbol_function_name(sym_key) };

    if let Some((raw_get, raw_set)) =
        super::class_own_symbol_accessor_ptrs(class_id, sym_key, false)
    {
        let get = scope.root_nanbox_f64(super::class_accessor_function_value(
            raw_get,
            false,
            false,
            &display_name,
            None,
        ));
        let set = scope.root_nanbox_f64(super::class_accessor_function_value(
            raw_set,
            true,
            false,
            &display_name,
            None,
        ));
        let proto_value =
            proto_h.with_mut_ptr::<ObjectHeader, _>(|p| crate::value::js_nanbox_pointer(p as i64));
        unsafe {
            crate::symbol::set_symbol_accessor_property(
                proto_value,
                sym_h.get_nanbox_f64(),
                if raw_get == 0 {
                    0
                } else {
                    get.get_nanbox_u64()
                },
                if raw_set == 0 {
                    0
                } else {
                    set.get_nanbox_u64()
                },
            );
        }
    } else if let Some((func_ptr, param_count, has_rest)) =
        super::class_own_symbol_method(class_id, sym_key, false)
    {
        let value = scope.root_nanbox_f64(crate::object::build_symbol_bound_method_closure(
            crate::object::class_prototype_ref_value(class_id),
            func_ptr,
            param_count,
            has_rest,
            false,
            &display_name,
        ));
        let proto_value =
            proto_h.with_mut_ptr::<ObjectHeader, _>(|p| crate::value::js_nanbox_pointer(p as i64));
        unsafe {
            crate::symbol::define_symbol_data_property(
                proto_value,
                sym_h.get_nanbox_f64(),
                value.get_nanbox_f64(),
            );
        }
    } else {
        return;
    }

    let owner = proto_h.with_mut_ptr::<ObjectHeader, _>(|p| p as usize);
    crate::symbol::set_symbol_property_attrs(owner, sym_key, PropertyAttrs::new(true, false, true));
}

fn install_class_decl_prototype_symbol_members(proto: *mut ObjectHeader, class_id: u32) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto_h = scope.root_raw_mut_ptr(proto);
    for sym_key in super::class_own_symbol_member_keys(class_id, false) {
        proto_h.with_mut_ptr(|p: *mut ObjectHeader| {
            install_class_decl_prototype_symbol_member(p, class_id, sym_key)
        });
    }
}

/// The class's own string-keyed prototype members in ClassBody order, each
/// with whether it is an accessor (else a method).
pub(crate) fn class_prototype_member_names(class_id: u32) -> Vec<(String, bool)> {
    let (mut names, accessors) = super::declarations::class_declared_member_names(class_id);
    names.extend(accessors.iter().cloned());
    order_class_string_member_names(class_id, false, &mut names);
    names
        .into_iter()
        // A private `#x` member is never a property of the prototype.
        .filter(|name| !name.starts_with('#'))
        .map(|name| {
            let is_accessor = accessors.contains(&name);
            (name, is_accessor)
        })
        .collect()
}

/// Install the class's own members on its decl prototype in ClassBody order:
/// methods as data properties, accessors as accessor properties (S2,
/// `decl_accessors.rs`).
fn install_class_decl_prototype_method_fields(proto: *mut ObjectHeader, class_id: u32) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto_h = scope.root_raw_mut_ptr(proto);
    for (name, is_accessor) in class_prototype_member_names(class_id) {
        // Both installers root the prototype across their own allocations.
        proto_h.with_mut_ptr(|proto: *mut ObjectHeader| {
            if is_accessor {
                super::decl_accessors::install_decl_prototype_accessor(proto, class_id, &name);
            } else {
                install_class_decl_prototype_method_field(proto, class_id, &name);
            }
        });
    }
}

/// The decl prototype `proto` of `class_id`, complete and linked: each method
/// was claimed with its attributes and then stored, and the link restamped
/// it, neither of which carries a lane. Its shape names each method's own
/// body, as an ordinary key-add of the same function object would: the slot
/// is a ConstFn lane (step 5C), and a later store of anything else to it
/// generalizes the lane through the store check.
fn learn_decl_prototype_method_lanes(proto: *mut ObjectHeader, class_id: u32) {
    let home = super::super::class_prototype_ref_value(class_id).to_bits();
    unsafe {
        crate::object::shapes::learn_object_constfn_lanes(proto, |_, bits| {
            class_method_entry_object_of(bits, home)
        });
    }
}

/// Is `bits` a function object running a class method's closure-convention
/// entry whose one capture is `home`?
unsafe fn class_method_entry_object_of(bits: u64, home: u64) -> bool {
    let value = JSValue::from_bits(bits);
    if !value.is_pointer() {
        return false;
    }
    let closure = value.as_pointer::<crate::closure::ClosureHeader>();
    crate::closure::is_closure_ptr(closure as usize)
        && crate::closure::real_capture_count((*closure).capture_count) >= 1
        && crate::closure::js_closure_get_capture_bits(closure, 0) == home
}

fn class_parent_prototype_bits(value: f64) -> Option<u64> {
    let bits = value.to_bits();
    if bits == crate::value::TAG_NULL {
        return Some(bits);
    }
    if !unsafe { super::super::object_ops::value_is_object_like(value) } {
        return None;
    }
    (unsafe { crate::symbol::js_is_symbol(value) } == 0).then_some(bits)
}

/// #10599: resolve the real `.prototype` object for a RESERVED native-builtin
/// parent class id -- one `builtin_parent_reserved_class_id` (perry-codegen)
/// wires as a class-registry parent edge for a native base that has no
/// declared-class registration of its own (`class Sub extends EventEmitter
/// {}` has no `js_register_class_name` call for `EventEmitter`). Without this,
/// `class_decl_prototype_value` bails immediately for such an id
/// (`class_name_for_id` returns `None`), so `Sub.prototype`'s `[[Prototype]]`
/// silently fell through to `Object.prototype` instead of
/// `EventEmitter.prototype` -- `Object.getPrototypeOf(Sub.prototype) !==
/// EventEmitter.prototype`, even though `new Sub() instanceof EventEmitter`
/// (a different mechanism -- the class-chain walk in `js_instanceof`) already
/// worked.
///
/// Scoped to the ids whose only registered subclassing surface is this
/// generic declared-class-prototype path: EventEmitter and its
/// AsyncResource variant, both bound as ordinary native-module callable
/// exports (`bound_native_callable_export_value`) whose own `.prototype` is
/// the same lazily-materialized, closure-identity-keyed object any bound
/// function's `.prototype` read produces
/// (`js_function_prototype_value_for_read`). Resolving through that exact
/// helper -- the same one the dynamic-parent branch below already uses for a
/// runtime function-valued superclass -- is what makes
/// `Object.getPrototypeOf(Sub.prototype) === EventEmitter.prototype` hold by
/// identity, not merely by shape. Array/Map/Set/Error/typed-array subclasses
/// have their own dedicated instance/prototype modeling and don't reach this
/// fallback the same way.
pub(crate) fn reserved_native_parent_prototype_bits(parent_id: u32) -> Option<u64> {
    let web = match parent_id {
        crate::native_class_ids::EVENT_TARGET => Some("EventTarget"),
        crate::native_class_ids::EVENT => Some("Event"),
        crate::native_class_ids::CUSTOM_EVENT => Some("CustomEvent"),
        crate::native_class_ids::ABORT_CONTROLLER => Some("AbortController"),
        crate::native_class_ids::ABORT_SIGNAL => Some("AbortSignal"),
        crate::native_class_ids::DOM_EXCEPTION => Some("DOMException"),
        _ => None,
    };
    if let Some(name) = web {
        return class_parent_prototype_bits(super::super::builtin_prototype_value(name));
    }
    const CLASS_ID_EVENT_EMITTER: u32 = 0xFFFF0076;
    const CLASS_ID_EVENT_EMITTER_ASYNC_RESOURCE: u32 = 0xFFFF0077;
    let (module, symbol) = match parent_id {
        // G1: the classic stream bases (perry-codegen
        // `builtin_parent_reserved_class_id`), whose prototypes carry the
        // stream methods.
        0xFFFF0071 => ("stream", "Readable"),
        0xFFFF0072 => ("stream", "Writable"),
        0xFFFF0073 => ("stream", "Duplex"),
        0xFFFF0074 => ("stream", "Transform"),
        0xFFFF0075 => ("stream", "PassThrough"),
        CLASS_ID_EVENT_EMITTER => ("events", "EventEmitter"),
        CLASS_ID_EVENT_EMITTER_ASYNC_RESOURCE => ("events", "EventEmitterAsyncResource"),
        crate::native_class_ids::ASYNC_LOCAL_STORAGE_LEGACY => ("async_hooks", "AsyncLocalStorage"),
        crate::native_class_ids::ASYNC_RESOURCE_LEGACY => ("async_hooks", "AsyncResource"),
        crate::native_class_ids::GZIP => ("zlib", "Gzip"),
        crate::native_class_ids::GUNZIP => ("zlib", "Gunzip"),
        crate::native_class_ids::DEFLATE => ("zlib", "Deflate"),
        crate::native_class_ids::INFLATE => ("zlib", "Inflate"),
        crate::native_class_ids::DEFLATE_RAW => ("zlib", "DeflateRaw"),
        crate::native_class_ids::INFLATE_RAW => ("zlib", "InflateRaw"),
        crate::native_class_ids::UNZIP => ("zlib", "Unzip"),
        crate::native_class_ids::BROTLI_COMPRESS => ("zlib", "BrotliCompress"),
        crate::native_class_ids::BROTLI_DECOMPRESS => ("zlib", "BrotliDecompress"),
        crate::native_class_ids::ZSTD_COMPRESS => ("zlib", "ZstdCompress"),
        crate::native_class_ids::ZSTD_DECOMPRESS => ("zlib", "ZstdDecompress"),
        _ => return None,
    };
    let func_value =
        super::super::native_module::bound_native_callable_export_value(module, symbol);
    let parent_proto = super::function_prototype::js_function_prototype_value_for_read(func_value);
    class_parent_prototype_bits(parent_proto)
}

/// `AsyncResource.prototype` as a NaN-boxed value, resolved exactly the way
/// the subclass edge above resolves it, so a direct `new AsyncResource(...)`
/// instance and a subclass prototype reach the SAME object by identity.
/// Returns `undefined` if the export is not materialized.
pub(crate) fn async_resource_prototype_value() -> f64 {
    let func_value = super::super::native_module::bound_native_callable_export_value(
        "async_hooks",
        "AsyncResource",
    );
    super::function_prototype::js_function_prototype_value_for_read(func_value)
}

/// `AsyncLocalStorage.prototype` from its bound native constructor export.
pub fn async_local_storage_prototype_value() -> f64 {
    let func_value = super::super::native_module::bound_native_callable_export_value(
        "async_hooks",
        "AsyncLocalStorage",
    );
    super::function_prototype::js_function_prototype_value_for_read(func_value)
}

/// Does CLASS identity `class_id` stand for a class with a prototype surface
/// (a declaration, or a registered runtime class)? An object literal's
/// anon-shape id is registered for `typeof` and may carry literal methods,
/// but its [[Prototype]] is `Object.prototype`: it has no declaration holder,
/// publishes no CLASS word and stays on ordinary property lookup. A declared
/// class whose id collides with an anon shape keeps its name.
pub(crate) fn class_identity_has_surface(class_id: u32) -> bool {
    class_id != 0
        && if super::is_anon_shape_class_id(class_id) {
            super::class_meta::class_has_name(class_id)
        } else {
            is_class_id_registered(class_id) || super::class_meta::class_has_name(class_id)
        }
}

pub(crate) fn class_decl_prototype_value(class_id: u32) -> f64 {
    // #7757: a specialization answers with its generic's prototype.
    class_decl_prototype_value_selected(decl_prototype_identity_id(class_id))
}

/// Continue the same prototype read after its outer generic-origin selection.
/// The caller selects immediately before reading the raw CLASS word, without
/// callbacks in between. Inner public projections deliberately remain intact:
/// a chained alias and a parent getter retain their existing semantics.
pub(crate) fn class_decl_prototype_value_selected(class_id: u32) -> f64 {
    if class_id == crate::wasi::CLASS_ID_WASI {
        crate::wasi::ensure_wasi_prototype_for_subclass();
        let proto = class_prototype_object(class_id);
        return (!proto.is_null())
            .then(|| crate::value::js_nanbox_pointer(proto as i64))
            .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
    }
    if !class_identity_has_surface(class_id) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

    // The class's function object carries the link, and minting it builds
    // the prototype through this function (its `prototype` own property), so
    // mint it first: a prototype is built once per class per agent, by the
    // innermost of the two calls.
    crate::object::class_value::class_value_ptr(class_id);
    let existing = class_decl_prototype_object(class_id);
    if !existing.is_null() {
        return crate::value::js_nanbox_pointer(existing as i64);
    }

    // The [[Prototype]] it links to, resolved before the object exists (a
    // parent that cannot be one throws before anything is linked).
    let scope = crate::gc::RuntimeHandleScope::new();
    let parent_proto_bits = decl_prototype_parent_bits(class_id);
    let parent_proto = scope.root_nanbox_u64(parent_proto_bits.unwrap_or(0));
    let existing = class_decl_prototype_object(class_id);
    if !existing.is_null() {
        return crate::value::js_nanbox_pointer(existing as i64);
    }
    if parent_proto_bits.is_some() {
        if let Some(proto) = super::decl_prototype_birth::decl_prototype_born_final(
            class_id,
            parent_proto.get_nanbox_u64(),
        ) {
            return proto;
        }
    }

    // Inline room for `constructor` and every declared member, so the
    // members' slots (an accessor's pair included) are inline: an inherited
    // read of one is then served by the inherited-read cache (spilled slots
    // never prime there).
    let members = class_prototype_member_names(class_id).len() as u32;
    let proto = scope.root_raw_mut_ptr(crate::object::js_object_alloc(class_id, members + 1));
    if proto.get_raw_mut_ptr::<ObjectHeader>().is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // Materializing a declared class's prototype object is not prototype
    // surgery: the object is fresh and unobserved, and the writes below
    // install `constructor` plus exactly the members the class declares. It
    // retires nothing (#7769: when it did, 384,000 of 384,000 shape-guard
    // probes on `gc-handoff/apps/shapes.ts` failed here).
    proto.with_mut_ptr::<ObjectHeader, _>(|p| link_decl_prototype_object(class_id, p));

    let constructor_key = scope.root_string_ptr(crate::string::js_string_from_bytes(
        b"constructor".as_ptr(),
        "constructor".len() as u32,
    ));
    let constructor = class_constructor_ref_value(class_id);
    proto.with_mut_ptr::<ObjectHeader, _>(|p| {
        constructor_key.with_const_ptr::<crate::StringHeader, _>(|key| {
            define_builtin_data_property(
                p,
                key,
                constructor,
                "constructor".to_string(),
                PropertyAttrs::new(true, false, true),
            );
        });
    });
    proto.with_mut_ptr(|p| install_class_decl_prototype_method_fields(p, class_id));
    // A computed member the definition has not named yet stops the
    // installation above; naming it resumes there.
    super::declarations::note_decl_prototype_born(class_id);
    proto.with_mut_ptr(|p| install_class_decl_prototype_symbol_members(p, class_id));

    if parent_proto_bits.is_some() {
        proto.with_mut_ptr::<ObjectHeader, _>(|p| {
            super::super::prototype_chain::object_set_static_prototype(
                p as usize,
                parent_proto.get_nanbox_u64(),
            );
        });
    }

    // Birth installs declared facts, not prototype mutations. Only the
    // completed holder becomes marked: its first ordinary store must retire
    // direct-call guards, while these initialization stores must preserve them.
    proto.with_mut_ptr::<ObjectHeader, _>(|p| unsafe {
        crate::object::proto_validity::mark_object_as_prototype(p as usize)
    });
    proto.with_mut_ptr(|p| learn_decl_prototype_method_lanes(p, class_id));
    proto.with_mut_ptr::<ObjectHeader, _>(|p| crate::value::js_nanbox_pointer(p as i64))
}

/// Read and validate one evaluation's superclass prototype with ordinary Get.
/// No template prototype, memo or first-evaluation state can answer this read.
pub(crate) fn evaluated_superclass_prototype(parent: f64) -> f64 {
    if parent.to_bits() == crate::value::TAG_NULL {
        return parent;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let parent = scope.root_nanbox_f64(parent);
    let key = crate::string::canonical_key(b"prototype");
    let parent_value = parent.get_nanbox_f64();
    let value = if let Some(cid) = super::super::class_ref_id(parent_value) {
        class_decl_prototype_value(cid)
    } else {
        super::super::field_get_set::js_object_get_field_by_name_f64(
            crate::value::js_nanbox_get_pointer(parent_value) as *const ObjectHeader,
            key,
        )
    };
    class_parent_prototype_bits(value)
        .map(f64::from_bits)
        .unwrap_or_else(|| {
            super::super::object_ops::throw_object_type_error(
                b"Class extends value does not have valid prototype property",
            )
        })
}

/// Definition-time preparation for a fresh class evaluation. The resolved
/// prototype travels through rooted generated operands into the new object.
#[no_mangle]
pub extern "C" fn js_class_evaluation_parent_prototype(class_id: u32, parent: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let parent = scope.root_nanbox_f64(parent);
    super::parent_static::register_class_parent_dynamic(class_id, parent.get_nanbox_f64(), false);
    evaluated_superclass_prototype(parent.get_nanbox_f64())
}

/// The [[Prototype]] of declared class `class_id`'s prototype object: its
/// parent class's prototype, the evaluated parent's, a runtime function
/// parent's `.prototype`, `null` for `extends null`, or `Object.prototype`.
/// Throws when a function-valued parent has no valid `.prototype`.
fn decl_prototype_parent_bits(class_id: u32) -> Option<u64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let dynamic_parent = scope.root_nanbox_f64(js_get_dynamic_parent_value(class_id));
    let null_heritage = dynamic_parent.get_nanbox_f64().to_bits() == crate::value::TAG_NULL;
    if null_heritage {
        // A class extending null creates a prototype object whose
        // [[Prototype]] is null, not Object.prototype.  Record TAG_NULL
        // explicitly so "no custom link" is not mistaken for the ordinary
        // Object.prototype default.
        Some(crate::value::TAG_NULL)
    } else {
        // A dynamically evaluated class has its own prototype object even
        // when it shares a template class id with other evaluations. Use the
        // parent VALUE recorded at this class definition, before consulting
        // the template's parent-id edge. The latter loses assignments such as
        // Effect's `Base.prototype.name = tag` on the actual parent object.
        let evaluated_parent_proto = {
            let parent_value = dynamic_parent.get_nanbox_f64();
            if super::is_class_object_value(parent_value) {
                let parent_obj = crate::value::JSValue::from_bits(parent_value.to_bits())
                    .as_pointer::<ObjectHeader>();
                let parent_proto = unsafe {
                    super::super::field_get_set::class_object_prototype_value(parent_obj)
                };
                class_parent_prototype_bits(f64::from_bits(parent_proto.bits()))
            } else {
                let parent = JSValue::from_bits(parent_value.to_bits());
                if parent.is_pointer()
                    && crate::closure::is_closure_ptr(parent.as_pointer::<u8>() as usize)
                {
                    Some(evaluated_superclass_prototype(parent_value).to_bits())
                } else {
                    None
                }
            }
        };
        let parent_proto = evaluated_parent_proto.or_else(|| {
            get_parent_class_id(class_id)
                .filter(|parent_id| *parent_id != 0 && *parent_id != class_id)
                .and_then(|parent_id| {
                    let parent_proto = class_decl_prototype_value(parent_id);
                    let parent_bits = parent_proto.to_bits();
                    if (parent_bits >> 48) == 0x7FFD {
                        return Some(parent_bits);
                    }
                    // #10599: `parent_id` may be a RESERVED native-builtin class id
                    // rather than a declared class -- `builtin_parent_reserved_class_id`
                    // in perry-codegen wires this edge for `class Sub extends
                    // EventEmitter {}`, which has no `js_register_class_name`
                    // registration of its own. `class_decl_prototype_value` bails
                    // immediately for such an id (`class_name_for_id` is `None`), so
                    // without this fallback the lookup above always misses and
                    // execution falls through to the runtime-function-valued branch
                    // below, which also misses (there is no dynamic-parent VALUE for
                    // a statically-resolved reserved id) -- landing `Sub.prototype`'s
                    // `[[Prototype]]` on `Object.prototype` instead of
                    // `EventEmitter.prototype`.
                    reserved_native_parent_prototype_bits(parent_id)
                })
        });
        parent_proto.or_else(global_object_prototype_bits)
    }
}

pub(crate) fn class_decl_prototype_value_for_instance_class(class_id: u32) -> Option<f64> {
    if !class_identity_has_surface(class_id) {
        return None;
    }
    let proto = class_decl_prototype_value(class_id);
    ((proto.to_bits() >> 48) == 0x7FFD).then_some(proto)
}

pub(crate) fn global_object_prototype_bits() -> Option<u64> {
    crate::object::object_prototype_intrinsic_bits()
}

#[cfg(test)]
mod class_realm_isolation_tests {
    use super::*;
    use std::sync::{Arc, Barrier, Mutex};

    /// #8001 — a declared class's lazily materialized `.prototype` belongs to
    /// the agent that materialized it, not to whichever thread first used the
    /// process-wide class id.
    ///
    /// The class id and name are codegen facts shared by both agents. The heap
    /// object stored under that id is not: each thread owns a separate realm and
    /// arena. Bootstraps are serialized because the test harness's globalThis
    /// root slot is process-global, while the barrier keeps both arenas live
    /// until both addresses have been observed. Before #8001, agent B returned
    /// agent A's cached pointer at the first-thread-wins gate.
    #[test]
    fn a_second_agents_declared_prototype_address_is_its_own() {
        const CLASS_ID: u32 = 0x7d01_8001;
        const CLASS_NAME: &[u8] = b"Issue8001Class";

        unsafe {
            js_register_class_name(CLASS_ID, CLASS_NAME.as_ptr(), CLASS_NAME.len() as u32);
        }

        let bootstrap_gate = Arc::new(Mutex::new(()));
        let both_alive = Arc::new(Barrier::new(2));
        let agent = |gate: Arc<Mutex<()>>, barrier: Arc<Barrier>| {
            move || -> usize {
                let addr = {
                    let _serialized = gate.lock().expect("bootstrap gate");
                    let bits = class_decl_prototype_value(CLASS_ID).to_bits();
                    assert_eq!(
                        bits & crate::value::TAG_MASK,
                        crate::value::POINTER_TAG,
                        "the class prototype must materialize before isolation is tested"
                    );
                    (bits & crate::value::POINTER_MASK) as usize
                };
                barrier.wait();
                addr
            }
        };
        let spawn_agent = |gate: Arc<Mutex<()>>, barrier: Arc<Barrier>| {
            std::thread::Builder::new()
                .stack_size(16 << 20)
                .spawn(agent(gate, barrier))
                .expect("spawn realm agent")
        };

        let a = spawn_agent(Arc::clone(&bootstrap_gate), Arc::clone(&both_alive));
        let b = spawn_agent(bootstrap_gate, both_alive);
        let a_addr = a.join().expect("agent A panicked");
        let b_addr = b.join().expect("agent B panicked");

        for (label, addr) in [("agent A", a_addr), ("agent B", b_addr)] {
            assert_ne!(
                addr, 0,
                "{label} did not materialize a real class prototype; distinctness would be vacuous"
            );
        }
        assert_ne!(
            a_addr, b_addr,
            "two live agents must materialize their own Class.prototype objects (#8001)"
        );
    }
}

#[cfg(test)]
mod class_dynamic_prop_store_tests {
    use super::*;

    fn stored(class_id: u32, name: &str) -> Option<f64> {
        class_own_static_field_value(class_id, name)
    }

    /// The in-place update arm must be observationally identical to the
    /// insert arm — same table, same value, same key set. This is the shape
    /// `Shape.made = Shape.made + 1` produces once per construction.
    #[test]
    fn repeated_store_updates_in_place_and_stays_readable() {
        let cid = 0x7c01_0001;
        for i in 0..5u32 {
            class_dynamic_prop_root_store(cid, "made", f64::from(i));
            assert_eq!(stored(cid, "made"), Some(f64::from(i)));
        }
        // A second key on the same class still inserts.
        class_dynamic_prop_root_store(cid, "other", 9.0);
        assert_eq!(stored(cid, "other"), Some(9.0));
        assert_eq!(stored(cid, "made"), Some(4.0));
        let mut keys = class_own_enumerable_field_names(cid);
        keys.sort();
        assert_eq!(keys, vec!["made".to_string(), "other".to_string()]);
    }

    /// `delete C.k` removes the key from the class function object; a later
    /// store defines it again.
    #[test]
    fn store_after_delete_defines_the_key_again() {
        let cid = 0x7c01_0002;
        class_dynamic_prop_root_store(cid, "k", 1.0);
        class_delete_own_dynamic_prop(cid, "k");
        assert_eq!(stored(cid, "k"), None);

        class_dynamic_prop_root_store(cid, "k", 2.0);
        assert_eq!(stored(cid, "k"), Some(2.0));

        class_dynamic_prop_root_store(cid, "k", 3.0);
        assert_eq!(stored(cid, "k"), Some(3.0));
    }
}

#[cfg(test)]
mod class_parent_prototype_tests {
    use super::*;

    #[test]
    fn only_object_and_null_parent_prototypes_are_valid() {
        let object_ptr = crate::object::js_object_alloc(0, 0);
        assert!(!object_ptr.is_null());
        let object = crate::value::js_nanbox_pointer(object_ptr as i64);
        assert_eq!(class_parent_prototype_bits(object), Some(object.to_bits()));
        assert_eq!(
            class_parent_prototype_bits(f64::from_bits(crate::value::POINTER_TAG | 0x1234)),
            None
        );
        assert_eq!(
            class_parent_prototype_bits(f64::from_bits(crate::value::TAG_NULL)),
            Some(crate::value::TAG_NULL)
        );
        assert_eq!(
            class_parent_prototype_bits(f64::from_bits(crate::value::TAG_UNDEFINED)),
            None
        );
        assert_eq!(class_parent_prototype_bits(1.0), None);
    }

    /// #8760: a `class X extends <bound native EventEmitter export>` registered
    /// through the dynamic-parent path (`import { EventEmitter as EE } from
    /// "events"; class X extends EE {}` — cli.js 2.1.112's exact shape) must
    /// link its declared prototype to EventEmitter's real, lazily-materialized
    /// prototype instead of throwing "Class extends value does not have valid
    /// prototype property". The raw `prototype` dynamic slot on the bound export
    /// closure is still `undefined` here, so the resolution must fall through to
    /// the same lazy materialization an ordinary `EE.prototype` read uses.
    #[test]
    fn native_constructor_export_parent_links_declared_prototype() {
        const ISSUE_8760_CLASS_ID: u32 = 0x7d01_8760;
        const CLASS_NAME: &[u8] = b"Issue8760Subclass";

        // The bound `events.EventEmitter` export — a BOUND_METHOD closure whose
        // `.prototype` object is materialized on demand, not at mint time.
        let ee_ctor = crate::object::bound_native_callable_export_value("events", "EventEmitter");

        // Precondition: the raw dynamic `prototype` slot is undefined — the exact
        // condition that made the dynamic-parent registration throw before the fix.
        let ee_addr = (ee_ctor.to_bits() & crate::value::POINTER_MASK) as usize;
        assert_eq!(
            crate::closure::closure_get_dynamic_prop(ee_addr, "prototype").to_bits(),
            crate::value::TAG_UNDEFINED,
            "precondition: the bound export's raw prototype slot must be undefined"
        );

        unsafe {
            js_register_class_name(
                ISSUE_8760_CLASS_ID,
                CLASS_NAME.as_ptr(),
                CLASS_NAME.len() as u32,
            );
        }
        js_register_class_parent_dynamic(ISSUE_8760_CLASS_ID, ee_ctor);

        // Must not throw, and must materialize a real prototype object.
        let decl_proto = class_decl_prototype_value(ISSUE_8760_CLASS_ID);
        assert_eq!(
            decl_proto.to_bits() & crate::value::TAG_MASK,
            crate::value::POINTER_TAG,
            "the subclass prototype must materialize (no TypeError) for a native constructor parent"
        );

        // Its [[Prototype]] must be EventEmitter's canonical prototype — the same
        // object an ordinary `EE.prototype` read resolves to — so `instanceof`
        // and inherited `emit`/`on` work through the chain.
        let ee_proto = super::function_prototype::ordinary_function_prototype_value_for_read(
            crate::object::bound_native_callable_export_value("events", "EventEmitter"),
        )
        .expect("EventEmitter export must expose a prototype object");
        let decl_proto_addr = (decl_proto.to_bits() & crate::value::POINTER_MASK) as usize;
        let linked = super::super::prototype_chain::object_static_prototype(decl_proto_addr)
            .expect("the subclass prototype must have a linked [[Prototype]]");
        assert_eq!(
            linked & crate::value::POINTER_MASK,
            ee_proto.to_bits() & crate::value::POINTER_MASK,
            "the subclass prototype must inherit from EventEmitter.prototype"
        );
    }
}
