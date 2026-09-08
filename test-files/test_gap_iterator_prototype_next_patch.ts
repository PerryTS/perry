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

// Node's console formatter itself iterates arrays. Buffer each line while the
// iterator prototypes are patched, then print once all originals are restored;
// otherwise the oracle crashes inside node:internal/per_context/primordials.
const output: string[] = [];
function log(...args: any[]) {
  output.push(args.join(" "));
}

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
    log("A-forof", got.join(","));
    log("A-spread", [...[4, 5]].join(","));
    log("A-from-array", Array.from([6]).join(","));
    log("A-from-iterator", Array.from([7].values()).join(","));
    const dynamicForOf = (value: any) => {
      const dynamic: number[] = [];
      for (const v of value) dynamic.push(v);
      return dynamic.join(",");
    };
    log("A-dynamic-forof", dynamicForOf([8, 9]));
    const capture = (...values: number[]) => values.join(",");
    log("A-call-spread", capture(...[10, 11]));
    const target = [0];
    target.push(...[12, 13]);
    log("A-push-spread", target.join(","));
    function pushLocalArray() {
      const local = [0];
      local.push(...[14, 15]);
      return local.join(",");
    }
    log("A-push-local", pushLocalArray());
    function pushLocalSet() {
      const local = [0];
      local.push(...new Set([22, 23]));
      return local.join(",");
    }
    log("A-push-set", pushLocalSet());
    function argumentsSpread(this: any) {
      return capture(...(arguments as any));
    }
    log("A-arguments-spread", argumentsSpread(16, 17));
    log("A-set-constructor", [...new Set([18, 19])].join(","));
    const it = [20, 21].values();
    log("A-manual", it.next().value, it.next().value, it.next().done);
  },
);

// B: after the restore the builtin is back, in every driver.
{
  const got: number[] = [];
  for (const v of [1, 2, 3]) got.push(v);
  log("B-forof", got.join(","));
  log("B-spread", [...[4, 5]].join(","));
  const it = [7, 8].values();
  log("B-manual", it.next().value, it.next().value, it.next().done);
  const holeTarget = [0];
  holeTarget.push(...[1, , 3]);
  log("B-push-holes", holeTarget.length, 2 in holeTarget, holeTarget[2] === undefined);
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
    log("C-forof-empty", got.length);
  },
);
log("C-restored", [...[9]].join(","));

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
    log("D-map", got.join(","));
    log("D-map-spread", [...new Map([["c", 3]])].join(","));
    log("D-map-from", Array.from(new Map([["d", 4]])).join(","));
  },
);
log("D-map-restored", [...new Map([["a", 1]])].join(","));
withPatched(
  setProto,
  (orig) =>
    function (this: any) {
      const r = orig.call(this);
      if (!r.done) r.value = "s" + r.value;
      return r;
    },
  () => {
    log("D-set", [...new Set([1, 2])].join(","));
    log("D-set-from", Array.from(new Set([3, 4])).join(","));
  },
);
log("D-set-restored", [...new Set([3])].join(","));

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
    log("E-string", [..."ab"].join(","));
    log("E-string-from", Array.from("cd").join(","));
  },
);
log("E-string-restored", [..."cd"].join(","));

// F: restoring by assigning the very same closure object, then a patch that
// is a bound copy of the original (same algorithm, different function
// object) — the proof compares by builtin entry, so the bound copy must NOT
// be mistaken for the builtin: its `this` is fixed to a different iterator.
{
  const orig = arrayProto.next;
  arrayProto.next = orig;
  log("F-same-object", [...[1, 2]].join(","));
  const other = [100, 200].values();
  arrayProto.next = orig.bind(other);
  try {
    log("F-bound-copy", [...[1, 2]].join(","));
  } finally {
    arrayProto.next = orig;
  }
  log("F-restored", [...[3]].join(","));
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
    log("G-accessor", [...[1, 2]].join(","), gets > 0);
  } finally {
    Object.defineProperty(arrayProto, "next", {
      value: orig,
      writable: true,
      enumerable: false,
      configurable: true,
    });
  }
  log("G-restored", [...[4]].join(","));
}

// H: a deleted prototype `next` makes for-of throw a TypeError; restoring
// it by plain assignment brings the builtin back.
{
  const orig = arrayProto.next;
  delete arrayProto.next;
  try {
    for (const _v of [1]) {
      log("H-unexpected");
    }
    log("H", "no-throw");
  } catch (e: any) {
    log("H", e instanceof TypeError);
  } finally {
    arrayProto.next = orig;
  }
  log("H-restored", [...[5, 6]].join(","));
}

// I: a NON-CALLABLE prototype `next` must throw a TypeError, not be mistaken
// for a pointer. The allocation-free proof reads the own slot as a raw value
// first, so a number, a string and `undefined` each have to defeat it.
for (const bad of [42, "not a function", undefined, null, {}]) {
  const orig = arrayProto.next;
  arrayProto.next = bad;
  try {
    for (const _v of [1]) {
      log("I-unexpected");
    }
    log("I", typeof bad, "no-throw");
  } catch (e: any) {
    log("I", typeof bad, e instanceof TypeError);
  } finally {
    arrayProto.next = orig;
  }
}
log("I-restored", [...[7, 8]].join(","));
console.log(output.join("\n"));
