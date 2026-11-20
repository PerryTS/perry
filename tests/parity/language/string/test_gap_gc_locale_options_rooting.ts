// parity-env: PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=4 PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_VERIFY_EVACUATION=1
// Runtime string entries that read `locales` / `options` (#12539). Reading
// them runs user code -- option getters, ToString on locale-list entries,
// array-like `length` / index getters -- and every hook below runs a polling
// loop, so a collection happens INSIDE the read. The strings themselves are
// fresh (young) heap strings, so a moving collection evacuates them.
//
// * localeCompare(that, locales, options): the runtime read `options.numeric`
//   a SECOND time (after the Collator validation already read it) while it
//   held the receiver and `that` as raw StringHeader addresses. Node reads
//   each option exactly once; the read counters below check that too. The
//   `numeric` getter churns only on its even reads, so on the old double-read
//   path the collection lands in the second read, while both strings are
//   still young and held raw.
// * toLocaleUpperCase / toLocaleLowerCase(locales): the runtime took the
//   receiver as a raw address and then resolved `locales`. Resolution skipped
//   object entries and array-likes entirely (no ToString, no Get), so the
//   Turkish/Lithuanian mappings were lost; CanonicalizeLocaleList runs them,
//   which makes the window real.
// * The Collator resolution itself held `options` raw while CanonicalizeLocaleList
//   ran `toString` on a locale entry, and across each GetOption getter.
// * Dynamic dispatch (method name not known at compile time): substr ran
//   ToIntegerOrInfinity(start) with the receiver AND the raw `length` object
//   held, and charAt/at/charCodeAt/codePointAt coerced the position after the
//   receiver address was taken.
//
// Under the from-space quarantine a stale address faults on first use.

function churn(rounds: number): number {
  let n = 0;
  for (let r = 0; r < rounds; r++) {
    const a: any[] = new Array(16);
    for (let j = 0; j < 16; j++) a[j] = { j, r, s: "v" + j };
    n += a.length;
  }
  return n;
}

// Longer than an inline short string: young heap strings.
const fresh = (i: number): string => "item-" + String(10 + i) + "-receiver-abcdefghijklmnop";
const other = (i: number): string => "item-" + String(9 - i) + "-receiver-abcdefghijklmnop";

let numericReads = 0;
let sensitivityReads = 0;
let localeToString = 0;
const numericOpts: any = {
  get numeric() {
    numericReads++;
    if (numericReads % 2 === 0) churn(400);
    return true;
  },
};
const sensOpts: any = {
  get sensitivity() {
    sensitivityReads++;
    churn(400);
    return "variant";
  },
};
const enList: any[] = [
  {
    toString() {
      localeToString++;
      churn(400);
      return "en";
    },
  },
];

for (let i = 0; i < 3; i++) {
  // numeric collation: "item-10-..." > "item-9-..." only when numeric is honored.
  console.log("cmp numeric", fresh(i).localeCompare(other(i), undefined, numericOpts));
  console.log("cmp locale+opts", fresh(i).localeCompare(other(i), enList, numericOpts));
  console.log("cmp sensitivity", fresh(i).localeCompare(other(i), "en", sensOpts));
  console.log("cmp locale only", fresh(i).localeCompare(other(i), enList));
}
console.log("numeric reads", numericReads);
console.log("sensitivity reads", sensitivityReads);
console.log("locale toString", localeToString);

// Dynamic dispatch: the method name is a runtime value, so the call goes
// through the runtime's string method table (native_call_method), not the
// inline lowering.
const M: string[] = [
  "localeCompare",
  "toLocaleUpperCase",
  "toLocaleLowerCase",
  "substr",
  "charAt",
  "at",
  "charCodeAt",
  "codePointAt",
];
function call1(s: any, m: string, a: any): any {
  return s[m](a);
}
function call2(s: any, m: string, a: any, b: any): any {
  return s[m](a, b);
}
function call3(s: any, m: string, a: any, b: any, c: any): any {
  return s[m](a, b, c);
}
let lcIdx = 0;
for (let k = 0; k < M.length; k++) if (M[k].length === 13) lcIdx = k;
const lc = M[lcIdx];
for (let i = 0; i < 3; i++) {
  console.log("dyn cmp numeric", call3(fresh(i), lc, other(i), undefined, numericOpts));
  console.log("dyn cmp locale", call3(fresh(i), lc, other(i), enList, sensOpts));
}
console.log("numeric reads", numericReads);

let trReads = 0;
const trList: any[] = [
  {
    toString() {
      trReads++;
      churn(400);
      return "tr";
    },
  },
];
const trLike: any = {
  get length() {
    trReads++;
    churn(400);
    return 1;
  },
  get 0() {
    trReads++;
    churn(400);
    return "tr";
  },
};
const ltList: any[] = [
  {
    toString() {
      churn(400);
      return "lt";
    },
  },
];
const istanbul = (i: number): string => "istanbul-" + String(i) + "-iiiiiiiiiiiiiiiiiiii";
const ISTANBUL = (i: number): string => "ISTANBUL-" + String(i) + "-IIIIIIIIIIIIIIIIIIII";
for (let i = 0; i < 3; i++) {
  console.log("upper tr list", istanbul(i).toLocaleUpperCase(trList));
  console.log("lower tr list", ISTANBUL(i).toLocaleLowerCase(trList));
  console.log("upper tr array-like", istanbul(i).toLocaleUpperCase(trLike));
  console.log("lower tr array-like", ISTANBUL(i).toLocaleLowerCase(trLike));
  console.log("lower lt list", ("J́-" + String(i) + "-JJJJJJJJJJJJJJJJJJJJ").toLocaleLowerCase(ltList));
}
console.log("tr reads", trReads);

const up = M[1];
const lo = M[2];
for (let i = 0; i < 3; i++) {
  console.log("dyn upper tr", call1(istanbul(i), up, trList));
  console.log("dyn lower tr", call1(ISTANBUL(i), lo, trLike));
}
console.log("tr reads", trReads);

// Errors still surface after the hooks ran (CanonicalizeLocaleList order).
try {
  istanbul(0).toLocaleUpperCase([{ toString() { churn(400); return "not_a_locale"; } }]);
} catch (e) {
  console.log("bad tag", (e as Error).name);
}
try {
  istanbul(0).toLocaleUpperCase([1 as any]);
} catch (e) {
  console.log("non-string tag", (e as Error).name);
}
try {
  fresh(0).localeCompare(other(0), undefined, { get usage() { churn(400); return "bogus"; } } as any);
} catch (e) {
  console.log("bad usage", (e as Error).name);
}

// substr through dynamic dispatch coerces start/length in the runtime.
const startObj: any = { valueOf() { churn(400); return 5; } };
const lenObj: any = { valueOf() { churn(400); return 4; } };
const sb = M[3];
for (let i = 0; i < 3; i++) {
  console.log("dyn substr", call2(fresh(i), sb, startObj, lenObj));
}
for (let k = 4; k < M.length; k++) {
  for (let i = 0; i < 2; i++) {
    console.log("dyn " + M[k], call1(fresh(i), M[k], startObj));
  }
}
