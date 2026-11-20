// #5897 (test262 built-ins/RegExp worklist): three RegExp-surface gaps.
//
// 1. An OWN property must SHADOW the `RegExp.prototype` method. Ordinary
//    `[[Get]]` consults the receiver's own properties before walking the
//    prototype chain, so an assigned `toString` / `exec` / `test` wins over the
//    builtin. Perry ignored the override: `exec`/`test` because the regex
//    method-dispatch arm ran before any own-property check, and `toString`
//    additionally because codegen folds every `x.toString()` straight into
//    `js_jsvalue_to_string_method` (test262 S15.10.4.1_A6_T1).
//
// 2. `RegExp`'s [[Prototype]] is `Function.prototype`, so a property added
//    there is readable off the constructor. The HIR member-lowering collapsed
//    `RegExp.<unknown>` to `globalThis.<unknown>`, dropping the receiver
//    (test262 S15.10.5_A2_T2).
//
// 3. JS string indices are UTF-16 code units, not Unicode scalars. The regex
//    module counted `chars()`, so every index at or past an astral character
//    was under-reported and disagreed with `str.length` / `charAt` on the same
//    string (test262 prototype/exec/u-lastindex-value).
//
// Validated byte-for-byte against `node --experimental-strip-types`.

// --- 1. own-property shadowing -------------------------------------------
const re: any = /a/g;
re.toString = Object.prototype.toString;
console.log("own toString  :", re.toString());

const re2: any = /a/g;
re2.toString = function () {
  return "OWN-TS";
};
console.log("own toString2 :", re2.toString());

const re3: any = /a/g;
re3.exec = function () {
  return "OWN-EXEC";
};
re3.test = function () {
  return "OWN-TEST";
};
console.log("own exec      :", re3.exec("a"));
console.log("own test      :", re3.test("a"));

// A regex WITHOUT an override still gets the builtin methods.
const plain = /a+/;
console.log("builtin toStr :", plain.toString());
console.log("builtin exec  :", JSON.stringify(plain.exec("caaat")));
console.log("builtin test  :", plain.test("caaat"));

// The brand is unaffected by an own toString.
console.log("brand         :", Object.prototype.toString.call(re2));

// --- 2. RegExp inherits from Function.prototype ---------------------------
(Function.prototype as any).indicator = 1;
console.log("RegExp.indic  :", (RegExp as any).indicator);
console.log("proto is FnP  :", Object.getPrototypeOf(RegExp) === Function.prototype);
// The intrinsic RegExp surfaces still resolve.
console.log("RegExp.proto  :", typeof RegExp.prototype);
console.log("RegExp.length :", RegExp.length);
console.log("RegExp.name   :", RegExp.name);

// --- 3. UTF-16 code-unit indices -----------------------------------------
// U+1D306 is ONE scalar but TWO UTF-16 code units.
const astral = "\u{1D306}";
console.log("length        :", astral.length);

const u = /./gu;
u.exec(astral);
console.log("lastIndex     :", u.lastIndex);

// Walking two astral scalars lands on 2 then 4.
const u2 = /./gu;
const two = "\u{1D306}\u{1D306}";
u2.exec(two);
console.log("walk 1        :", u2.lastIndex);
u2.exec(two);
console.log("walk 2        :", u2.lastIndex);
console.log("walk 3 (null) :", u2.exec(two));
console.log("reset         :", u2.lastIndex);

// `.index` and `search` agree with `indexOf` past an astral char.
const m = /x/.exec(astral + "x");
console.log("match index   :", m ? m.index : -1);
console.log("search        :", (astral + "x").search(/x/));
console.log("indexOf       :", (astral + "x").indexOf("x"));

// The offset handed to a replace callback is a JS string index too.
(astral + "x").replace(/x/, (match: string, offset: number) => {
  console.log("replace offset:", offset);
  return match;
});

// BMP text is unchanged.
const bmp = /o/g;
bmp.exec("foo");
console.log("bmp lastIndex :", bmp.lastIndex);
