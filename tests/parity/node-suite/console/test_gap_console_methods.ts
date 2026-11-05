// Test missing console methods: table, dir, time/timeEnd/timeLog, group/groupEnd, assert, count/countReset, trace, clear

// === console.table ===
// Verify it doesn't crash; output format varies by runtime
console.table([{ a: 1, b: 2 }, { a: 3, b: 4 }]);
console.log("table: ok"); // table: ok

// Table with array of arrays
console.table([[1, 2], [3, 4]]);
console.log("table arrays: ok"); // table arrays: ok

// Table with single object
console.table({ name: "Alice", age: 30 });
console.log("table object: ok"); // table object: ok

// === console.dir ===
console.dir({ nested: { obj: true, arr: [1, 2, 3] } });
console.log("dir: ok"); // dir: ok

// Dir with options (depth)
console.dir({ a: { b: { c: { d: 1 } } } }, { depth: 2 });
console.log("dir depth: ok"); // dir depth: ok

// === console.time / console.timeEnd / console.timeLog ===
console.time("timer1");

// Do some work
let total = 0;
for (let i = 0; i < 100000; i++) {
  total += i;
}

// timeLog prints intermediate time
console.timeLog("timer1");
console.log("timeLog: ok"); // timeLog: ok

// More work
for (let i = 0; i < 100000; i++) {
  total += i;
}

// timeEnd prints final time and removes the timer
console.timeEnd("timer1");
console.log("timeEnd: ok"); // timeEnd: ok

// Multiple independent timers
console.time("timerA");
console.time("timerB");
console.timeEnd("timerB");
console.timeEnd("timerA");
console.log("multiple timers: ok"); // multiple timers: ok

// === console.group / console.groupEnd ===
console.group("outer group");
console.log("inside outer"); // inside outer
console.group("inner group");
console.log("inside inner"); // inside inner
console.groupEnd();
console.log("back to outer"); // back to outer
console.groupEnd();
console.log("group: ok"); // group: ok

// Collapsed group (just verify no crash)
console.groupCollapsed("collapsed");
console.log("inside collapsed"); // inside collapsed
console.groupEnd();
console.log("groupCollapsed: ok"); // groupCollapsed: ok

// === console.assert ===
// True assertion produces no output
console.assert(true, "this should not appear");
console.log("assert true: ok"); // assert true: ok

// False assertion prints the message (to stderr)
console.assert(false, "assertion failed message");
console.log("assert false: ok"); // assert false: ok

// Assert with multiple arguments
console.assert(false, "failed with", 42, "items");
console.log("assert multi: ok"); // assert multi: ok

// Assert with no message
console.assert(false);
console.log("assert no msg: ok"); // assert no msg: ok

// === console.count / console.countReset ===
console.count("myCounter");   // myCounter: 1
console.count("myCounter");   // myCounter: 2
console.count("myCounter");   // myCounter: 3
console.log("count: ok"); // count: ok

console.countReset("myCounter");
console.count("myCounter");   // myCounter: 1 (reset back to 1)
console.log("countReset: ok"); // countReset: ok

// Default label
console.count();              // default: 1
console.count();              // default: 2
console.log("count default: ok"); // count default: ok

// === console.trace ===
// Prints a stack trace (output varies, just verify no crash)
console.trace("trace message");
console.log("trace: ok"); // trace: ok

// === console.clear ===
// No-op in non-TTY environments, just verify no crash
console.clear();
console.log("clear: ok"); // clear: ok

// === console.debug / console.info ===
// These are aliases for console.log in most environments
console.debug("debug message");
console.log("debug: ok"); // debug: ok

console.info("info message");
console.log("info: ok"); // info: ok

console.log("All console method tests passed!"); // All console method tests passed!

// Note: Expected output is approximate because console.table, console.dir,
// console.time*, console.trace, console.assert(false, ...) have varying formats.
// The key assertions are the "ok" markers that confirm no crashes occurred.
//
// Deterministic expected lines (ignoring table/dir/time/trace/assert output):
// table: ok
// table arrays: ok
// table object: ok
// dir: ok
// dir depth: ok
// timeLog: ok
// timeEnd: ok
// multiple timers: ok
// inside outer
// inside inner
// back to outer
// group: ok
// inside collapsed
// groupCollapsed: ok
// assert true: ok
// assert false: ok
// assert multi: ok
// assert no msg: ok
// myCounter: 1
// myCounter: 2
// myCounter: 3
// count: ok
// countReset: ok
// myCounter: 1
// count default: ok
// default: 1
// default: 2
// trace: ok (after trace output)
// clear: ok
// debug message
// debug: ok
// info message
// info: ok
// All console method tests passed!
