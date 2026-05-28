// Regression: `new Error(value)` with a non-string `value` (number, null,
// object, undefined, boolean) used to SIGSEGV inside
// `js_error_new_with_message` because perry's codegen handed the raw value
// straight to the runtime, and the runtime then deref'd the bogus pointer
// at `(*message).byte_len`. effect's Cause.ts exercises this path through
// its bug-error pretty printers. The defensive `is_valid_obj_ptr` gate in
// `alloc_error` now coerces a non-pointer `message` to an empty string,
// so all of these must succeed without crashing — the resulting `.message`
// matches Node's coerced toString where the value is a real string, and
// is "" where we have to fall back to the empty-string default.
//
// (#321 frontier issue #69, mirrors #2230's `is_valid_obj_ptr` pattern.)

// 1. integer
const eNum = new Error(123 as unknown as string);
console.log("num:", typeof eNum.message);
console.log("num-instanceof:", eNum instanceof Error);

// 2. null
const eNull = new Error(null as unknown as string);
console.log("null:", typeof eNull.message);
console.log("null-instanceof:", eNull instanceof Error);

// 3. plain object literal
const eObj = new Error({ a: 1 } as unknown as string);
console.log("obj:", typeof eObj.message);
console.log("obj-instanceof:", eObj instanceof Error);

// 4. undefined arg (zero-arg ctor)
const eUndef = new Error();
console.log("undef:", typeof eUndef.message);
console.log("undef-instanceof:", eUndef instanceof Error);

// 5. boolean
const eBool = new Error(true as unknown as string);
console.log("bool:", typeof eBool.message);
console.log("bool-instanceof:", eBool instanceof Error);

// 6. baseline: real string still works
const eStr = new Error("real message");
console.log("str:", eStr.message);
console.log("str-instanceof:", eStr instanceof Error);

// Expected output:
// num: string
// num-instanceof: true
// null: string
// null-instanceof: true
// obj: string
// obj-instanceof: true
// undef: string
// undef-instanceof: true
// bool: string
// bool-instanceof: true
// str: real message
// str-instanceof: true
