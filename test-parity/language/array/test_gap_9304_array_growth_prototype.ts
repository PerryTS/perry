// parity-env: PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_PROTECT_FROMSPACE=1
// #9304: replacing an ArrayHeader during dense growth must carry its recorded
// [[Prototype]] entry to the new owner address. The element copy already kept
// the indexed values and length, but the address-keyed prototype registry was
// left on the forwarding stub, so the live array silently regained the default
// Array.prototype chain.
//
// Moving GC replaces the owner address through the same layout-transfer hook
// and must also trace a custom prototype value. Exercise both mechanisms.

declare function gc(): void;

const nulled: any = [4, 5];
const nulledAlias: any = nulled;
Object.setPrototypeOf(nulled, null);
console.log("null before:", Object.getPrototypeOf(nulled) === null, typeof nulled.map);
if (typeof gc === "function") gc();
nulled[9] = 1;
console.log(
  "null after indexed grow:",
  Object.getPrototypeOf(nulled) === null,
  Object.getPrototypeOf(nulledAlias) === null,
  typeof nulled.map,
  typeof nulledAlias.map,
);
console.log("null indexed value:", nulled.length, nulled[0], nulled[9]);

// The registry transfer is prototype-value agnostic: preserve an object
// prototype and its inherited surface too, including over a second growth.
const custom: any = { marker: "kept" };
const retargeted: any = [7, 8];
Object.setPrototypeOf(retargeted, custom);
if (typeof gc === "function") gc();
retargeted[9] = 2;
retargeted[20] = 3;
console.log(
  "object after indexed grows:",
  Object.getPrototypeOf(retargeted) === custom,
  retargeted.marker,
  typeof retargeted.map,
  retargeted.length,
  retargeted[20],
);

// Array.prototype.push.call reaches the same reallocation primitive even when
// the custom chain deliberately makes receiver.push unavailable.
const pushed: any = [10, 11, 12, 13];
Object.setPrototypeOf(pushed, null);
if (typeof gc === "function") gc();
console.log("borrowed push length:", Array.prototype.push.call(pushed, 14));
console.log(
  "null after push grow:",
  Object.getPrototypeOf(pushed) === null,
  typeof pushed.push,
  pushed.length,
  pushed[4],
);
