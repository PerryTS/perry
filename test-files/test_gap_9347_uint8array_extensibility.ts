// A BufferHeader-backed Uint8Array misses lookup_typed_array_kind. Its
// extensibility state must use the same TypedArray side table as every other
// kind, or Object/Reflect report true and allow new properties after a
// successful preventExtensions call.
function inspectReflect(label: string, value: Uint8Array | Int32Array): void {
  (value as any).existing = 1;
  console.log(
    label,
    "create",
    Reflect.set(value, "created", 7),
    (value as any).created,
    Object.prototype.hasOwnProperty.call(value, "created"),
  );
  console.log(label, "before", Object.isExtensible(value), Reflect.isExtensible(value));
  console.log(label, "prevent", Reflect.preventExtensions(value));
  console.log(label, "after", Object.isExtensible(value), Reflect.isExtensible(value));
  console.log(
    label,
    "set",
    Reflect.set(value, "existing", 2),
    (value as any).existing,
    Reflect.set(value, "fresh", 3),
    Object.prototype.hasOwnProperty.call(value, "fresh"),
  );
  console.log(
    label,
    "define",
    Reflect.defineProperty(value, "late", { value: 4 }),
    Object.prototype.hasOwnProperty.call(value, "late"),
  );
}

function inspectObject(label: string, value: Uint8Array | Int32Array): void {
  Object.preventExtensions(value);
  console.log(
    label,
    Object.isExtensible(value),
    Reflect.isExtensible(value),
    Reflect.set(value, "fresh", 3),
    Object.prototype.hasOwnProperty.call(value, "fresh"),
  );
}

inspectReflect("reflect-u8", new Uint8Array(2));
inspectReflect("reflect-i32", new Int32Array(2));
inspectObject("object-u8", new Uint8Array(2));
inspectObject("object-i32", new Int32Array(2));
