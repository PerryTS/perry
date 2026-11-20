// #5586: RegExp.prototype.exec lastIndex semantics (ECMA-262 22.2.7.2
// RegExpBuiltinExec).
//
// Two spec details that Perry previously got wrong:
//
//  1. Step 4 reads `lastIndex` (Get → ToLength) exactly ONCE, up front, BEFORE
//     the global/sticky branch (step 8). So a coercible `lastIndex` is observed
//     once even for a non-global/non-sticky regex — and is NOT written back.
//     (test262 prototype/exec/{success,failure}-lastindex-access)
//
//  2. The lastIndex updates use `Set(R, "lastIndex", v, true)` (Throw=true), so
//     a non-writable `lastIndex` makes a stateful match raise a TypeError
//     instead of silently dropping the write.
//     (test262 prototype/{exec,test}/y-fail-lastindex-no-write)

function ok(name: string, value: boolean) {
  if (!value) {
    throw new Error(name + ": FAIL");
  }
  console.log(name + ": ok");
}

// (1) lastIndex is read exactly once for a non-global/non-sticky regex, and the
//     property is left untouched (no write-back).
let gets = 0;
const counter = {
  valueOf() {
    gets++;
    return 0;
  },
};
const r = /./;
(r as any).lastIndex = counter;
const res = r.exec("abc");
ok("nonglobal-match", res !== null && res![0] === "a");
ok("nonglobal-lastindex-read-once", gets === 1);
ok("nonglobal-lastindex-not-written", (r as any).lastIndex === counter);

// Same for a non-matching non-global regex: one read, no write.
gets = 0;
const r2 = /a/;
(r2 as any).lastIndex = counter;
ok("nonglobal-nomatch-null", r2.exec("nbc") === null);
ok("nonglobal-nomatch-read-once", gets === 1);
ok("nonglobal-nomatch-not-written", (r2 as any).lastIndex === counter);

// (2) A global regex DOES advance lastIndex, and resets it to 0 on failure.
const g = /a/g;
ok("global-1", g.exec("banana")!.index === 1 && g.lastIndex === 2);
ok("global-2", g.exec("banana")!.index === 3 && g.lastIndex === 4);
ok("global-3", g.exec("banana")!.index === 5 && g.lastIndex === 6);
ok("global-reset", g.exec("banana") === null && g.lastIndex === 0);

// (3) Non-writable lastIndex + a stateful (sticky) match failure throws.
const y = /c/y;
Object.defineProperty(y, "lastIndex", { writable: false });
let threw = false;
try {
  y.exec("abc");
} catch (e) {
  threw = e instanceof TypeError;
}
ok("sticky-nonwritable-exec-throws", threw);

// `test` shares the same RegExpBuiltinExec path, so it throws too.
const y2 = /c/y;
Object.defineProperty(y2, "lastIndex", { writable: false });
threw = false;
try {
  y2.test("abc");
} catch (e) {
  threw = e instanceof TypeError;
}
ok("sticky-nonwritable-test-throws", threw);

console.log("done");
