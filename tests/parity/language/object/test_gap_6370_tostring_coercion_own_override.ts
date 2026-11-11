// #6370 — an own `toString` on an exotic instance (RegExp / Date) must win on
// EVERY ToString site, not just the explicit `x.toString()` call.
//
// Ordinary [[Get]] consults the receiver's own properties before the prototype
// chain, so `String(re)`, `` `${re}` ``, `[re].join("")`, `"".concat(re)` … all
// have to see an assigned/defined `toString` — data property OR accessor.
// Before the fix, `re.toString()` honoured the override (#6358) while the
// ToString coercion path mapped the regex straight back to its `/source/flags`
// literal, so the same regex stringified two different ways depending on how
// you asked. Date had the identical split.

function attempt(fn: () => string): string {
  try {
    return fn();
  } catch (e: any) {
    return "THROW: " + e.message;
  }
}

// ── RegExp, own toString as a DATA property ───────────────────────────────
const r1: any = /a/;
r1.toString = () => "DATA-toString";
console.log("re data .toString() :", r1.toString());
console.log("re data String()    :", String(r1));
console.log("re data template    :", `${r1}`);
console.log('re data "" + re     :', "" + r1);
console.log('re data re + ""     :', r1 + "");
console.log("re data join        :", [r1].join(""));
console.log("re data join sep    :", ["x", r1].join("-"));
console.log("re data arr toString:", [r1].toString());
console.log("re data concat      :", "".concat(r1));

// ── RegExp, own toString as an ACCESSOR ───────────────────────────────────
const r2: any = /b/;
Object.defineProperty(r2, "toString", {
  get() {
    return () => "ACC-toString";
  },
  configurable: true,
});
console.log("re acc  .toString() :", r2.toString());
console.log("re acc  String()    :", String(r2));
console.log("re acc  template    :", `${r2}`);
console.log('re acc  "" + re     :', "" + r2);

// ── RegExp with NO override still renders /source/flags everywhere ────────
const r3 = /c/gi;
console.log("re none String()    :", String(r3));
console.log("re none template    :", `${r3}`);
console.log('re none "" + re     :', "" + r3);
console.log("re none .toString() :", r3.toString());
console.log("re none join        :", [r3].join(""));

// ── A non-callable own toString SHADOWS the builtin: ToPrimitive throws ───
const r4: any = /d/;
r4.toString = 5;
console.log("re bad  String()    :", attempt(() => String(r4)));

// ── Hint matters: an own valueOf wins for "default", toString for "string" ─
const r5: any = /e/;
r5.valueOf = () => "VALUEOF";
console.log("re vOf  String()    :", String(r5));
console.log("re vOf  template    :", `${r5}`);
console.log('re vOf  re + ""     :', r5 + "");

// ── Symbol.toPrimitive takes precedence over toString on the coercion path ─
const r6: any = /f/;
r6[Symbol.toPrimitive] = (hint: string) => "SYMPRIM-" + hint;
console.log("re sym  String()    :", String(r6));
console.log("re sym  template    :", `${r6}`);

// ── Date: same own-property shadowing on the coercion path ────────────────
const d1: any = new Date(0);
d1.toString = () => "DATE-DATA";
console.log("date data .toString():", d1.toString());
console.log("date data String()   :", String(d1));
console.log("date data template   :", `${d1}`);
console.log('date data "" + d     :', "" + d1);

const d2: any = new Date(0);
Object.defineProperty(d2, "toString", {
  get() {
    return () => "DATE-ACC";
  },
  configurable: true,
});
console.log("date acc  String()   :", String(d2));
console.log("date acc  template   :", `${d2}`);

// A Date with no override still renders the built-in date string (kept
// timezone-independent so the fixture is stable everywhere).
const d3 = new Date(0);
console.log("date none builtin    :", String(d3) === d3.toString(), String(d3).includes("1970"));

// ── Error already honoured own overrides on both paths — regression guard ──
const e1: any = new Error("boom");
e1.toString = () => "ERR-DATA";
console.log("err data String()    :", String(e1));
console.log("err data template    :", `${e1}`);
const e2 = new TypeError("bad");
console.log("err none String()    :", String(e2));
