// Issue #536 regression: two perry-runtime/perry-stdlib bugs that broke
// `@perryts/mysql` (and any TS-source npm driver routing through
// `import 'net'` to perry-ext-net).
//
// (1) `[].pop()` and `[].shift()` returned `f64::NAN` instead of
//     `undefined` per ECMAScript §23.1.3.21 / §23.1.3.27. Connection-pool
//     drivers like `@perryts/mysql`'s pool `acquire()` did
//     `const entry = this.idle.shift(); if (entry !== undefined) { ... }`
//     and took the wrong branch on an empty pool because bare NaN bits
//     compare `!== undefined`.
//
// (2) `js_stdlib_has_active_handles` was gated on `feature = "net"`
//     (= bundled-net), so under the well-known flip that routes
//     `import 'net'` to perry-ext-net the runtime didn't see the
//     ext-net's open sockets and exited early — `await new Promise(
//     r => sock.on('connect', r))` never resolved (pre-fix the event
//     loop drained before any tokio worker delivered the connect event).

const empty: number[] = [];
const popped = empty.pop();
const shifted = empty.shift();

console.log('pop typeof:', typeof popped);
console.log('pop === undefined:', popped === undefined);
console.log('shift typeof:', typeof shifted);
console.log('shift === undefined:', shifted === undefined);

// Real-world shape from @perryts/mysql pool.acquire():
const idle: number[] = [];
const entry = idle.shift();
if (entry !== undefined) {
    console.log('FAIL: entry !== undefined was wrongly truthy');
} else {
    console.log('OK: empty.shift() === undefined matches Node.js spec');
}

// Non-empty array: should still return the head element, not undefined.
const xs = [11, 22, 33];
console.log('shift first:', xs.shift());   // 11
console.log('after shift length:', xs.length);  // 2
console.log('pop last:', xs.pop());        // 33
console.log('after pop length:', xs.length);    // 1
