// #9417: an accessor call must not corrupt the CALLER's `this` across an
// evacuating young-gen minor.
//
// `invoke_accessor_getter` / `invoke_accessor_setter` (perry-runtime
// `object/field_get_set/accessors.rs`) bind the getter's receiver by writing
// the GC-rooted `IMPLICIT_THIS` cell, keeping the previous occupant in a bare
// Rust local, and writing it back after the accessor body has run. The body is
// user code: it allocates, so a copying minor can relocate the caller's
// receiver in the middle. A bare Rust local is precisely the slot the collector
// cannot see or rewrite, so the restore reinstalled a PRE-collection address as
// the caller's `this`.
//
// Nothing crashes: `js_object_get_own_field_or_undef` fails its
// `obj_type == GC_TYPE_OBJECT` check on the retired cell and returns
// `undefined`, so the caller's next `this.<field>` silently answers
// `undefined`. claude-code hit this converting its tool schemas to JSON Schema
// on the unauthenticated path and reported
// `Cannot read properties of undefined (reading 'def')` instead of
// `Not logged in · Please run /login`.
//
// Pre-fix this printed `caller-this bad=30` (deterministically, no GC env
// knobs); node prints 0 for both lines.

const notes: string[] = [];
let bad = 0;

// Companion assertion: the getter's own `this`. This half already passed
// before the fix (the rebound closure carries a freshly-rooted `this`
// capture) — it pins the contract, it is not the demonstrated gap.
function makeHost(i: number) {
  const host: any = { id: i, inner: { def: "d" + i }, pad: "pad" + i };
  Object.defineProperty(host, "lazy", {
    get: function (this: any) {
      const tmp: any[] = [];
      for (let k = 0; k < 240; k++) tmp.push({ k: k, s: "t" + k, pad: [k, k + 1] });
      return this.inner.def + ":" + tmp.length;
    },
    configurable: true,
    enumerable: true,
  });
  return host;
}

for (let i = 0; i < 12000; i++) {
  const host = makeHost(i);
  const want = "d" + i + ":240";
  let got: any;
  try {
    got = host.lazy;
  } catch (e: any) {
    got = "THREW:" + (e && e.message);
  }
  if (got !== want) {
    bad++;
    if (bad <= 3) notes.push("[" + i + " got=" + String(got) + "]");
  }
}
console.log("getter-this bad=" + bad + notes.join(""));

// The demonstrated gap: the caller's `this` after the accessor returns.
const notes2: string[] = [];
let bad2 = 0;
const probe: any = {};
Object.defineProperty(probe, "lazy", {
  get: function () {
    const tmp: any[] = [];
    for (let k = 0; k < 240; k++) tmp.push({ k: k, s: "u" + k, pad: [k, k + 1] });
    return tmp.length;
  },
  configurable: true,
  enumerable: true,
});

function makeCaller(i: number) {
  return {
    id: i,
    inner: { def: "c" + i },
    run: function (this: any) {
      const n = probe.lazy;
      return this.inner.def + ":" + n;
    },
  };
}

for (let i = 0; i < 12000; i++) {
  const c: any = makeCaller(i);
  const want = "c" + i + ":240";
  let got: any;
  try {
    got = c.run();
  } catch (e: any) {
    got = "THREW:" + (e && e.message);
  }
  if (got !== want) {
    bad2++;
    if (bad2 <= 3) notes2.push("[" + i + " got=" + String(got) + "]");
  }
}
console.log("caller-this bad=" + bad2 + notes2.join(""));
