// #8426: `String.prototype.normalize` must not hold a borrowed heap-string
// payload across the form argument's ToString coercion.
//
// The coercion is a collection point twice over: an inline short-string form
// materializes onto the heap, and an object form runs user `toString`, whose
// loop back-edge polls can run a moving minor. Either can evacuate the
// subject; a `&str` taken beforehand then points into from-space.
//
// The fix reorders the coercion ahead of the borrow, so this file also pins
// the two observable orderings that reorder must NOT change: ToString runs
// before the form is validated, and a Symbol form throws TypeError (§7.1.17)
// rather than the invalid-form RangeError (#2782).

// Build the subject at runtime so it is a young *nursery* heap string (>5
// bytes, so not SSO) rather than a folded constant: "cafe" + U+0301 combining
// acute. `join` matters: a `+=` accumulator chain leaves its buffer outside the
// movable nursery, so a subject built that way never relocates and this test
// would pass whether or not the bug is present.
function buildSubject(tag: string): string {
  const acute = String.fromCharCode(0x0301);
  return ["caf", "e", acute, "-", tag].join("");
}

// Churn hard enough to cross several loop back-edge safepoint polls: volume
// alone is not enough, the collector needs garbage to actually move.
function churn(): void {
  let junk = "";
  const scraps: string[] = [];
  for (let i = 0; i < 5000; i++) {
    junk = junk + "x";
    if (i % 50 === 0) {
      scraps.push(junk.slice(0, 8) + i);
    }
  }
  if (junk.length !== 5000 || scraps.length !== 100) {
    throw new Error("string churn was optimized away");
  }
}

// ---- 1. the bug: subject must survive a moving collection in the window ----
let coercions = 0;
const subject = buildSubject("runtime");
const reentrantForm = {
  toString(): string {
    coercions++;
    churn();
    return "NFC";
  },
};
console.log("reentrant NFC =>", JSON.stringify(subject.normalize(reentrantForm as any)));
console.log("reentrant coercions =>", coercions);

// Decomposing form, same window — a different normalization pass over the
// same borrowed payload.
const subjectD = buildSubject("decompose");
const reentrantFormD = {
  toString(): string {
    churn();
    return "NFD";
  },
};
const decomposed = subjectD.normalize(reentrantFormD as any);
console.log("reentrant NFD length =>", decomposed.length);
console.log("reentrant NFD roundtrip =>", JSON.stringify(decomposed.normalize("NFC")));

// Repeat under sustained pressure: each call opens the window again.
let repeated = "";
for (let i = 0; i < 20; i++) {
  const s = buildSubject("iter" + i);
  repeated = s.normalize({
    toString(): string {
      churn();
      return "NFC";
    },
  } as any);
}
console.log("repeated last =>", JSON.stringify(repeated));

// A plain string form is the *common* case and still allocates (an SSO form
// materializes onto the heap inside the coercion).
console.log("sso form =>", JSON.stringify(buildSubject("sso").normalize("NFC")));

// ---- 2. ToString still runs BEFORE the form is validated ----
let badCoercions = 0;
try {
  buildSubject("bad").normalize({
    toString(): string {
      badCoercions++;
      churn();
      return "BAD";
    },
  } as any);
  console.log("bad form => no throw");
} catch (e: any) {
  console.log("bad form =>", e.name);
}
console.log("bad form coercions =>", badCoercions);

// ---- 3. a Symbol form throws TypeError, not RangeError (#2782) ----
try {
  buildSubject("sym").normalize(Symbol("nope") as any);
  console.log("symbol form => no throw");
} catch (e: any) {
  console.log("symbol form =>", e.name);
}
