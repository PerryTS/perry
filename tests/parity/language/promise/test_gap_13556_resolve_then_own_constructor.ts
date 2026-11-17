// Gap test for #13556 (test262 built-ins/Promise/resolve/arg-uniq-ctor): a
// promise whose own `constructor` is not %Promise% is not returned by
// `Promise.resolve`; the new promise's thenable job calls its `then`, whose
// SpeciesConstructor throws a TypeError for a non-object `constructor`, and
// that TypeError rejects the new promise. The fused `Promise.resolve(p).then(..)`
// lowering called `p.then` directly and threw the TypeError uncaught.
const p1 = Promise.resolve(1);
(p1 as any).constructor = 5;
Promise.resolve(p1).then(
  () => console.log("number constructor: fulfilled"),
  (e) => console.log("number constructor: rejected", e instanceof TypeError),
);

const p2 = new Promise<number>((r) => r(2));
(p2 as any).constructor = null;
const q2 = Promise.resolve(p2);
console.log("null constructor: same promise", q2 === p2);
q2.then(
  () => console.log("null constructor: fulfilled"),
  (e) => console.log("null constructor: rejected", e instanceof TypeError),
);

const p3 = Promise.resolve(3);
(p3 as any).constructor = Promise;
console.log("intrinsic constructor: same promise", Promise.resolve(p3) === p3);
Promise.resolve(p3).then((v) => console.log("intrinsic constructor: fulfilled", v));
