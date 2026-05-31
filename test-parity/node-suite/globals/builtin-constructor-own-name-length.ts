// #3655 — built-in *constructors* carry spec-correct own `name`/`length` data
// properties, observable through the full property protocol (not just the
// compile-time `Ctor.name`/`Ctor.length` folds, which #3143 already handled).
//
// Each constructor is read through a local `any` binding so nothing folds at
// compile time — this exercises the runtime value path and the reflection
// helpers (`getOwnPropertyDescriptor`, `hasOwnProperty`, `propertyIsEnumerable`,
// `getOwnPropertyNames`, the `in` operator, and `delete`).
//
// NOTE: test262's `verifyProperty` reaches these via
// `Function.prototype.call.bind(Object.prototype.hasOwnProperty)` (the
// "uncurry-this" idiom), which is a separate reification gap and is exercised
// through the DIRECT `.call` / method forms here instead.

function probe(o: any, label: string) {
  console.log("== " + label + " ==");
  console.log("typeof", typeof o);
  console.log("name desc", JSON.stringify(Object.getOwnPropertyDescriptor(o, "name")));
  console.log("length desc", JSON.stringify(Object.getOwnPropertyDescriptor(o, "length")));
  console.log("name val", JSON.stringify(o.name), "length val", o.length);

  // hasOwnProperty — both the method form and the direct `.call` form.
  console.log("method hasOwn name", o.hasOwnProperty("name"));
  console.log("call hasOwn length", Object.prototype.hasOwnProperty.call(o, "length"));
  console.log("call hasOwn missing", Object.prototype.hasOwnProperty.call(o, "nope"));

  // `in` operator.
  console.log("in name/length", "name" in o, "length" in o);

  // Non-enumerable: propertyIsEnumerable false, and for-in must skip them.
  console.log("pie name/length", o.propertyIsEnumerable("name"), o.propertyIsEnumerable("length"));
  let leaks = false;
  for (const k in o) {
    if (k === "name" || k === "length") leaks = true;
  }
  console.log("for-in leaks name/length", leaks);

  // getOwnPropertyNames includes the built-in slots in spec order.
  const names = Object.getOwnPropertyNames(o).filter(
    (k: string) => k === "name" || k === "length" || k === "prototype",
  );
  console.log("ownNames", JSON.stringify(names));

  // (writable:false is asserted via the descriptor above; a direct write is
  // skipped here because ESM strict mode throws on it — see #3143's test.)

  // configurable:true — delete removes the OWN property entirely. (A later
  // read would inherit `name` from the prototype chain; that fallback is out
  // of scope here, so only the own-property facts are asserted.)
  delete o.name;
  console.log("after delete hasOwn name", o.hasOwnProperty("name"));
  console.log("after delete desc", JSON.stringify(Object.getOwnPropertyDescriptor(o, "name")));

  // `prototype` is non-configurable: delete must fail (strict mode throws, so
  // catch it) and leave the property intact.
  let protoDeleted = true;
  try {
    protoDeleted = delete o.prototype;
  } catch {
    protoDeleted = false;
  }
  console.log("delete prototype", protoDeleted, "still own", o.hasOwnProperty("prototype"));
}

// A representative spread of constructor arities: 0 (Map), 1 (DataView/BigInt),
// 2 (RegExp), 7 (Date), and a TypedArray (3).
probe(Map as any, "Map");
probe(DataView as any, "DataView");
probe(BigInt as any, "BigInt");
probe(RegExp as any, "RegExp");
probe(Date as any, "Date");
probe(Uint8Array as any, "Uint8Array");
probe(Promise as any, "Promise");
probe(TypeError as any, "TypeError");

// A user function keeps its own writable name/length-free semantics: name &
// length are still own + non-enumerable + configurable, and user props stay
// enumerable + writable.
function userFn(a: number, b: number) {
  return a + b;
}
(userFn as any).tag = 1;
console.log("== userFn ==");
console.log("name/length", JSON.stringify(userFn.name), userFn.length);
console.log("hasOwn name/length/tag", userFn.hasOwnProperty("name"), userFn.hasOwnProperty("length"), userFn.hasOwnProperty("tag"));
console.log("pie name/tag", userFn.propertyIsEnumerable("name"), userFn.propertyIsEnumerable("tag"));
console.log("ownNames", JSON.stringify(Object.getOwnPropertyNames(userFn).filter((k: string) => k === "name" || k === "length" || k === "tag")));
delete (userFn as any).tag;
console.log("after delete tag hasOwn", userFn.hasOwnProperty("tag"));
