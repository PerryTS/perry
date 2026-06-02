// #764 regression: `const cell = State("")` at module-init level (NOT
// inside a function) used to get hijacked by perry-transform's
// state_desugar pass — rewritten to `__state_init("__state_0", ...)` while
// `stateOnChange(cell, ...)` was left calling against `undefined`, so
// `.set()` updated the keyed registry but no subscribers ever fired. The
// fix scopes the rewrite away from any binding that's consumed by a
// handle-based state API (stateOnChange / stateBindTextfield / …).
import { State, stateOnChange } from 'perry/ui';

const cell = State("initial");
console.log("Initial cell.value =", cell.value);

let callbackFired = 0;
let lastSeen = "";

stateOnChange(cell, (v: string) => {
    callbackFired += 1;
    lastSeen = v as string;
    console.log("[stateOnChange] fired with:", v);
});

console.log("Before set: callbackFired =", callbackFired);
cell.set("updated-once");
console.log("After 1st set: cell.value =", cell.value, "callbackFired =", callbackFired, "lastSeen =", lastSeen);
cell.set("updated-twice");
console.log("After 2nd set: cell.value =", cell.value, "callbackFired =", callbackFired, "lastSeen =", lastSeen);
