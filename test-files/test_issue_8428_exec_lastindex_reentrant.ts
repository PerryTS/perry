// parity-env: PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_SCHEDULE_SEED=8428 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1
//
// #8428: RegExpBuiltinExec step 4 is `ToLength(Get(R, "lastIndex"))`, and the
// ToNumber half of it runs user JS whenever `lastIndex` is a coercible object.
// `js_regexp_exec` used to borrow the subject's inline WTF-8 payload BEFORE
// that coercion, so a moving minor inside the user callback left the borrow
// pointing at from-space for the whole match. The callbacks below churn enough
// young strings to reach several back-edge safepoint polls.

function makeSubject(): string {
  // Built at runtime so the subject is a young heap string rather than a
  // constant baked into the binary.
  let subject = "prefix";
  subject = subject + "-young";
  subject = subject + "-42-suffix";
  return subject;
}

function movingLastIndex(label: string, result: number): object {
  return {
    valueOf(): number {
      let junk = label;
      for (let i = 0; i < 512; i++) {
        junk = junk + "x";
      }
      // Keep the churn observable so nothing above can be folded away.
      if (junk.length !== label.length + 512) {
        throw new Error("string churn was optimized away");
      }
      return result;
    },
  };
}

// (a) std-regex arm: the `regex` crate compiles this pattern.
const standardSubject = makeSubject();
const standard = /(young)-(\d+)/g;
standard.lastIndex = movingLastIndex("standard", 0) as any;
const standardMatch = standard.exec(standardSubject)!;
console.log(
  "standard",
  standardMatch[0],
  standardMatch[1],
  standardMatch[2],
  standardMatch.index,
  standardMatch.input,
  standard.lastIndex,
);

// (b) fancy-regex arm: the lookbehind forces the fancy fallback, which reads
// the same borrow through a different matcher.
const fancySubject = makeSubject();
const fancy = /(?<=prefix-)(young)-(\d+)/g;
fancy.lastIndex = movingLastIndex("fancy", 0) as any;
const fancyMatch = fancy.exec(fancySubject)!;
console.log(
  "fancy",
  fancyMatch[0],
  fancyMatch[1],
  fancyMatch[2],
  fancyMatch.index,
  fancyMatch.input,
  fancy.lastIndex,
);

// (c) a NON-zero coerced lastIndex additionally walks the borrow in
// `utf16_index_to_byte` to find the search start.
const offsetSubject = makeSubject();
const offset = /(\d+)/g;
offset.lastIndex = movingLastIndex("offset", 7) as any;
const offsetMatch = offset.exec(offsetSubject)!;
console.log("offset", offsetMatch[0], offsetMatch.index, offset.lastIndex);

// (d) named captures + `d` (hasIndices) exercise the groups/indices decoration
// built after the same borrow.
const namedSubject = makeSubject();
const named = /(?<word>young)-(?<num>\d+)/dg;
named.lastIndex = movingLastIndex("named", 0) as any;
const namedMatch = named.exec(namedSubject)!;
console.log(
  "named",
  namedMatch[0],
  namedMatch.groups!.word,
  namedMatch.groups!.num,
  JSON.stringify(namedMatch.indices),
  named.lastIndex,
);

// (e) `String.prototype.matchAll` reads `lastIndex` through the same coercion
// before it snapshots the subject.
const allSubject = makeSubject();
const all = /(\w+)-(\d+)/g;
all.lastIndex = movingLastIndex("all", 0) as any;
const allResults = Array.from(allSubject.matchAll(all)).map((m) => m[0]);
console.log("matchAll", allResults.join("|"));

// (f) `RegExp.prototype.test` routes global regexes through exec.
const testSubject = makeSubject();
const tester = /young/g;
tester.lastIndex = movingLastIndex("test", 0) as any;
console.log("test", tester.test(testSubject), tester.lastIndex);
