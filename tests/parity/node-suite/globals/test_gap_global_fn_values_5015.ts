// #5015 — callable global helpers used as first-class VALUES.
// queueMicrotask / structuredClone / atob / btoa were callable directly
// (`queueMicrotask(fn)`) but a bare value read (`const m = queueMicrotask`,
// `{ scheduleMicrotask: queueMicrotask }`) fell through to the GlobalGet(0)
// sentinel and evaluated to the number 0 — so `typeof` was "number" and
// calling the stored value threw "value is not a function". This is exactly
// what react-reconciler's host config (`scheduleMicrotask: queueMicrotask`)
// hit inside updateContainerSync → scheduleImmediateRootScheduleTask.

// Bare value reads: typeof must be "function".
const qm: any = queueMicrotask;
const sc: any = structuredClone;
const a: any = atob;
const b: any = btoa;
console.log(typeof qm, typeof sc, typeof a, typeof b);

// As object-literal property values (the react-reconciler host-config shape),
// then extracted and called.
const cfg: any = {
  scheduleMicrotask: queueMicrotask,
  clone: structuredClone,
  decode: atob,
  encode: btoa,
};
console.log(typeof cfg.scheduleMicrotask, typeof cfg.clone, typeof cfg.decode, typeof cfg.encode);

// Call through the stored value.
console.log(JSON.stringify(cfg.clone({ x: 1, y: [2, 3] })));
console.log(cfg.encode("hi"), cfg.decode(cfg.encode("hi")));
cfg.scheduleMicrotask(() => console.log("microtask ran"));

// Direct calls still work (these were always fine — guard against regressions).
console.log(btoa("ok"), atob("b2s="));
console.log(JSON.stringify(structuredClone([1, { a: 2 }])));
queueMicrotask(() => console.log("direct microtask ran"));
