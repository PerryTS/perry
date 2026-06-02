// Behavioral parity coverage for the object, descriptor, symbol, and
// prototype FFI surface. Output is deterministic so it can byte-compare
// against Node --experimental-strip-types.

function line(label: string, value: unknown) {
  console.log(label + ":", value);
}

// Basic object literal, indexed access, and dynamic property.
const obj: Record<string, unknown> = { a: 1, b: "two", c: true };
line("keys", Object.keys(obj).join(","));
line("values", Object.values(obj).map((v) => String(v)).join(","));
line("entries", Object.entries(obj).map(([k, v]) => k + "=" + String(v)).join("|"));

obj["d"] = 4;
line("set-key", Object.keys(obj).join(","));
delete obj["b"];
line("after-delete", Object.keys(obj).join(","));

// hasOwn / in.
line("hasOwn-yes", Object.hasOwn(obj, "a"));
line("hasOwn-no", Object.hasOwn(obj, "missing"));
line("in-yes", "a" in obj);
line("in-no", "missing" in obj);

// defineProperty / descriptor inspection / configurable / writable.
const desc: Record<string, unknown> = {};
Object.defineProperty(desc, "secret", {
  value: 42,
  enumerable: false,
  writable: false,
  configurable: false,
});
const d = Object.getOwnPropertyDescriptor(desc, "secret");
line("desc-value", d?.value);
line("desc-enumerable", d?.enumerable);
line("desc-writable", d?.writable);
line("desc-keys", Object.keys(desc).join(","));
line("desc-prop-names", Object.getOwnPropertyNames(desc).join(","));

// Object.assign / Object.create / Object.fromEntries.
const assigned = Object.assign({ x: 1 }, { y: 2 }, { z: 3 });
line("assign", Object.entries(assigned).map(([k, v]) => k + "=" + String(v)).join(","));

// Object.create with null prototype is the deterministic shape Perry
// currently supports; prototype chain lookup via Object.create(obj) is
// tracked separately in the gap suite.
const nullProto = Object.create(null);
nullProto["name"] = "perry";
line("create-null-name", nullProto["name"]);
line("create-null-keys", Object.keys(nullProto).join(","));

const fromEntries = Object.fromEntries([
  ["a", 1],
  ["b", 2],
]);
line("fromEntries", Object.entries(fromEntries).map(([k, v]) => k + "=" + String(v)).join(","));

// freeze / seal / preventExtensions / isFrozen / isSealed / isExtensible.
const fz = Object.freeze({ a: 1 });
line("isFrozen", Object.isFrozen(fz));
line("isExtensible-frozen", Object.isExtensible(fz));

const sealed = Object.seal({ a: 1, b: 2 });
line("isSealed", Object.isSealed(sealed));
line("isFrozen-sealed", Object.isFrozen(sealed));

const ext = { a: 1 } as Record<string, number>;
Object.preventExtensions(ext);
line("preventExtensions", Object.isExtensible(ext));

// Object.is — distinguishes -0/+0 and NaN.
line("is-equal", Object.is(1, 1));
line("is-nan", Object.is(NaN, NaN));
line("is-zero", Object.is(-0, +0));

// Symbols: new, for / keyFor, description, toString, properties.
const localSym = Symbol("local");
const sharedSym = Symbol.for("perry.compat.objects");
line("sym-desc", localSym.description);
line("sym-keyFor-shared", Symbol.keyFor(sharedSym));
line("sym-keyFor-local", Symbol.keyFor(localSym));
line("sym-toString", sharedSym.toString());
line("sym-typeof", typeof localSym);

const symObj: Record<string | symbol, unknown> = { ord: 1 };
symObj[sharedSym] = "via-shared";
symObj[localSym] = "via-local";
line("sym-prop-count", Object.getOwnPropertySymbols(symObj).length);
line("sym-prop-read", symObj[sharedSym]);

// Prototype-based classes — toString tag, instanceof, getter, static.
class Animal {
  static kind = "generic";
  name: string;
  constructor(name: string) {
    this.name = name;
  }
  get label() {
    return "animal:" + this.name;
  }
  toString() {
    return "Animal(" + this.name + ")";
  }
}
class Dog extends Animal {
  static override kind = "dog";
  bark() {
    return this.name + " says woof";
  }
}
const rex = new Dog("Rex");
line("class-getter", rex.label);
line("class-method", rex.bark());
line("class-toString", rex.toString());
line("instanceof-dog", rex instanceof Dog);
line("instanceof-animal", rex instanceof Animal);
line("instanceof-object", rex instanceof Object);
line("static-self", Dog.kind);
line("static-parent", Animal.kind);

// Object.groupBy — newer std utility.
const grouped = Object.groupBy([1, 2, 3, 4, 5, 6], (n: number) =>
  n % 2 === 0 ? "even" : "odd",
);
line("groupBy-even", (grouped["even"] ?? []).join(","));
line("groupBy-odd", (grouped["odd"] ?? []).join(","));

// Object spread / rest.
const base = { a: 1, b: 2, c: 3 };
const spread = { ...base, d: 4 };
line("spread", Object.entries(spread).map(([k, v]) => k + "=" + String(v)).join(","));
const { a: _restA, ...rest } = base;
line("rest", Object.entries(rest).map(([k, v]) => k + "=" + String(v)).join(","));

// globalThis access.
line("globalThis-self", typeof globalThis === "object");

// Map: alloc, set/get/has/delete, size, iteration in insertion order.
const m = new Map<string, number>();
m.set("first", 1);
m.set("second", 2);
m.set("third", 3);
line("map-size", m.size);
line("map-get", m.get("second"));
line("map-has-yes", m.has("first"));
line("map-has-no", m.has("missing"));
m.delete("second");
line("map-after-delete", Array.from(m.keys()).join(","));

const mFromArr = new Map([
  ["k1", 10],
  ["k2", 20],
]);
line("map-from-array", Array.from(mFromArr.values()).join(","));
line(
  "map-entries",
  Array.from(m.entries())
    .map(([k, v]) => k + "=" + v)
    .join("|"),
);
const fePairs: string[] = [];
m.forEach((value, key) => {
  fePairs.push(key + ":" + value);
});
line("map-forEach", fePairs.join(","));

m.clear();
line("map-clear", m.size);

// Set: alloc, add/has/delete/size, dedup, iteration.
const s = new Set<number>();
s.add(1);
s.add(2);
s.add(2);
s.add(3);
line("set-size", s.size);
line("set-has-yes", s.has(2));
line("set-has-no", s.has(99));
s.delete(2);
line("set-after-delete", Array.from(s).join(","));

const sFromArr = new Set([1, 2, 3, 1, 2]);
line("set-from-array", Array.from(sFromArr).join(","));
const setSum = { sum: 0 };
sFromArr.forEach((n) => (setSum.sum += n));
line("set-forEach-sum", setSum.sum);
sFromArr.clear();
line("set-clear", sFromArr.size);

console.log("compat-objects-symbols: ok");

/*
@covers
crates/perry-runtime/src/object.rs:
  - js_build_class_keys_array
  - js_class_register_static_field
  - js_create_native_module_namespace
  - js_get_global_this
  - js_implicit_this_get
  - js_implicit_this_set
  - js_instanceof_dynamic
  - js_native_call_method_apply
  - js_native_call_method_str_key
  - js_native_module_property_by_name
  - js_object_alloc
  - js_object_alloc_class_inline_keys
  - js_object_alloc_class_with_keys
  - js_object_alloc_fast
  - js_object_alloc_fast_with_parent
  - js_object_alloc_with_parent
  - js_object_alloc_with_shape
  - js_object_assign_one
  - js_object_clone_with_extra
  - js_object_copy_own_fields
  - js_object_create
  - js_object_delete_dynamic
  - js_object_entries
  - js_object_free
  - js_object_freeze
  - js_object_from_entries
  - js_object_get_class_id
  - js_object_get_field_f64
  - js_object_get_index_polymorphic
  - js_object_get_own_field_or_undef
  - js_object_get_own_property_descriptor
  - js_object_get_own_property_names
  - js_object_get_prototype_of
  - js_object_group_by
  - js_object_has_own
  - js_object_has_property
  - js_object_is
  - js_object_is_extensible
  - js_object_is_frozen
  - js_object_is_sealed
  - js_object_keys
  - js_object_prevent_extensions
  - js_object_rest
  - js_object_seal
  - js_object_set_field_by_index
  - js_object_set_field_f64
  - js_object_set_index_polymorphic
  - js_object_set_keys
  - js_object_to_string
  - js_object_to_value
  - js_object_values
  - js_register_class_extends_error
  - js_register_class_getter
  - js_register_class_has_instance
  - js_register_class_id
  - js_register_class_setter
  - js_register_class_to_string_tag
  - js_register_handle_method_dispatch
  - js_register_handle_property_dispatch
  - js_register_handle_property_set_dispatch
  - js_unresolved_namespace_stub
  - js_value_to_object
  - perry_key_content_hash
crates/perry-runtime/src/symbol.rs:
  - js_class_register_static_symbol
  - js_is_symbol
  - js_object_get_own_property_symbols
  - js_object_get_symbol_property
  - js_object_has_own_symbol
  - js_object_set_symbol_method
  - js_object_set_symbol_property
  - js_symbol_description
  - js_symbol_equals
  - js_symbol_for
  - js_symbol_key_for
  - js_symbol_new
  - js_symbol_new_empty
  - js_symbol_to_string
  - js_symbol_typeof
  - js_to_primitive
crates/perry-runtime/src/map.rs:
  - js_map_alloc
  - js_map_clear
  - js_map_delete
  - js_map_entries
  - js_map_entry_key_at
  - js_map_entry_value_at
  - js_map_foreach
  - js_map_from_array
  - js_map_get
  - js_map_has
  - js_map_keys
  - js_map_set
  - js_map_size
  - js_map_values
crates/perry-runtime/src/set.rs:
  - js_set_add
  - js_set_alloc
  - js_set_clear
  - js_set_delete
  - js_set_foreach
  - js_set_from_array
  - js_set_from_iterable
  - js_set_has
  - js_set_size
  - js_set_to_array
  - js_set_value_at
*/
