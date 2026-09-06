// A replaced `%ArrayIteratorPrototype%.next` (and the Map / Set / String
// family prototypes) must drive for-of, spread, Array.from and manual calls;
// restoring the ORIGINAL closure must hand iteration back to the builtin.
// The runtime proves "not patched" allocation-free by comparing the
// prototype's own `next` against the builtin thunk, so both directions of a
// replace / restore cycle are exercised on every family, plus an accessor
// `next` on the prototype, which that proof must decline.

const arrayProto: any = Object.getPrototypeOf([][Symbol.iterator]());
const mapProto: any = Object.getPrototypeOf(new Map().entries());
const setProto: any = Object.getPrototypeOf(new Set().values());
const stringProto: any = Object.getPrototypeOf(""[Symbol.iterator]());

function withPatched(proto: any, patch: (orig: any) => any, body: () => void) {
  const orig = proto.next;
  proto.next = patch(orig);
  try {
    body();
  } finally {
    proto.next = orig;
  }
}

// A: array family, every driver, doubled values through the patch.
withPatched(
  arrayProto,
  (orig) =>
    function (this: any) {
      const r = orig.call(this);
      if (!r.done) r.value = (r.value as number) * 2;
      return r;
    },
  () => {
    const got: number[] = [];
    for (const v of [1, 2, 3]) got.push(v);
    console.log("A-forof", got.join(","));
    console.log("A-spread", [...[4, 5]].join(","));
    console.log("A-from", Array.from([6].values()).join(","));
    const it = [7, 8].values();
    console.log("A-manual", it.next().value, it.next().value, it.next().done);
  },
);

// B: after the restore the builtin is back, in every driver.
{
  const got: number[] = [];
  for (const v of [1, 2, 3]) got.push(v);
  console.log("B-forof", got.join(","));
  console.log("B-spread", [...[4, 5]].join(","));
  const it = [7, 8].values();
  console.log("B-manual", it.next().value, it.next().value, it.next().done);
}

// C: a second replace after the restore is honoured again (the proof is a
// per-call read, not a one-shot latch).
withPatched(
  arrayProto,
  () =>
    function () {
      return { done: true, value: undefined };
    },
  () => {
    const got: number[] = [];
    for (const v of [1, 2]) got.push(v);
    console.log("C-forof-empty", got.length);
  },
);
console.log("C-restored", [...[9]].join(","));

// D: Map and Set family prototypes, patched and restored.
withPatched(
  mapProto,
  (orig) =>
    function (this: any) {
      const r = orig.call(this);
      if (!r.done) r.value = [r.value[0], (r.value[1] as number) + 100];
      return r;
    },
  () => {
    const got: string[] = [];
    for (const [k, v] of new Map([["a", 1], ["b", 2]])) got.push(k + "=" + v);
    console.log("D-map", got.join(","));
  },
);
console.log("D-map-restored", [...new Map([["a", 1]])].join(","));
withPatched(
  setProto,
  (orig) =>
    function (this: any) {
      const r = orig.call(this);
      if (!r.done) r.value = "s" + r.value;
      return r;
    },
  () => {
    console.log("D-set", [...new Set([1, 2])].join(","));
  },
);
console.log("D-set-restored", [...new Set([3])].join(","));

// E: String family prototype.
withPatched(
  stringProto,
  (orig) =>
    function (this: any) {
      const r = orig.call(this);
      if (!r.done) r.value = (r.value as string).toUpperCase();
      return r;
    },
  () => {
    console.log("E-string", [..."ab"].join(","));
  },
);
console.log("E-string-restored", [..."cd"].join(","));

// F: restoring by assigning the very same closure object, then a patch that
// is a bound copy of the original (same algorithm, different function
// object) — the proof compares by builtin entry, so the bound copy must NOT
// be mistaken for the builtin: its `this` is fixed to a different iterator.
{
  const orig = arrayProto.next;
  arrayProto.next = orig;
  console.log("F-same-object", [...[1, 2]].join(","));
  const other = [100, 200].values();
  arrayProto.next = orig.bind(other);
  try {
    console.log("F-bound-copy", [...[1, 2]].join(","));
  } finally {
    arrayProto.next = orig;
  }
  console.log("F-restored", [...[3]].join(","));
}

// G: an accessor `next` on the prototype is consulted on every step.
{
  const orig = arrayProto.next;
  let gets = 0;
  Object.defineProperty(arrayProto, "next", {
    configurable: true,
    get() {
      gets++;
      return orig;
    },
  });
  try {
    console.log("G-accessor", [...[1, 2]].join(","), gets > 0);
  } finally {
    Object.defineProperty(arrayProto, "next", {
      value: orig,
      writable: true,
      enumerable: false,
      configurable: true,
    });
  }
  console.log("G-restored", [...[4]].join(","));
}

// H: a deleted prototype `next` makes for-of throw a TypeError; restoring
// it by plain assignment brings the builtin back.
{
  const orig = arrayProto.next;
  delete arrayProto.next;
  try {
    for (const _v of [1]) {
      console.log("H-unexpected");
    }
    console.log("H", "no-throw");
  } catch (e: any) {
    console.log("H", e instanceof TypeError);
  } finally {
    arrayProto.next = orig;
  }
  console.log("H-restored", [...[5, 6]].join(","));
}
