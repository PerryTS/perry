// Timer callbacks on the web/wasm target (#1323).
// setTimeout / setInterval callbacks must actually fire, and
// clearInterval must stop a repeating timer. Without the emit-side
// bridge dispatch, these calls were silent no-ops: top-level code ran
// but no deferred callback ever executed.
console.log("top");

// setInterval fires, then clears itself via the captured timer id.
const id = setInterval(() => {
  console.log("tick");
  clearInterval(id);
}, 5);

// setTimeout fires after the interval has ticked once and cleared.
setTimeout(() => {
  console.log("done");
}, 40);
