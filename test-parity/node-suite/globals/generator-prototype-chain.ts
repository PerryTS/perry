// #4141: a generator/async-generator INSTANCE must sit on the spec prototype
// chain — `Object.getPrototypeOf(Object.getPrototypeOf(gen()))` resolves to
// `%Generator.prototype%` / `%AsyncGenerator.prototype%`, the object carrying
// the brand-checked `next`/`return`/`throw` with `configurable:true`
// descriptors. Complements globals/generator-intrinsics.ts (function-side).

function* g() {
  yield 1;
}
async function* ag() {
  yield 1;
}

function descOf(o: any, k: string) {
  const d = Object.getOwnPropertyDescriptor(o, k);
  return d
    ? { writable: d.writable, enumerable: d.enumerable, configurable: d.configurable }
    : undefined;
}

// --- instance two-hop chain reaches the brand-checked prototype ---
const SyncProto = Object.getPrototypeOf(Object.getPrototypeOf(g()));
console.log("sync inst proto has next:", typeof SyncProto.next);
console.log("sync next desc:", JSON.stringify(descOf(SyncProto, "next")));
console.log("sync return desc:", JSON.stringify(descOf(SyncProto, "return")));
console.log("sync throw desc:", JSON.stringify(descOf(SyncProto, "throw")));
console.log("sync constructor desc:", JSON.stringify(descOf(SyncProto, "constructor")));

const AsyncProto = Object.getPrototypeOf(Object.getPrototypeOf(ag()));
console.log("async inst proto has next:", typeof AsyncProto.next);
console.log("async next desc:", JSON.stringify(descOf(AsyncProto, "next")));
console.log("async constructor desc:", JSON.stringify(descOf(AsyncProto, "constructor")));

// --- the instance proto and %Generator.prototype% are distinct objects ---
console.log("sync two distinct hops:", Object.getPrototypeOf(g()) !== SyncProto);

// --- brand check via the instance-reached prototype ---
for (const bad of [undefined, null, {}, function () {}]) {
  try {
    SyncProto.next.call(bad as any);
    console.log("sync brand NO THROW");
  } catch (e) {
    console.log("sync brand throws TypeError:", e instanceof TypeError);
  }
}

// --- delegation: prototype method drives a real instance ---
console.log("sync delegated:", JSON.stringify(SyncProto.next.call(g())));

// --- async brand check rejects (queued last, deterministic) ---
AsyncProto.next.call(undefined as any).then(
  () => console.log("async brand NO REJECT"),
  (e: any) => console.log("async brand rejects TypeError:", e instanceof TypeError),
);
