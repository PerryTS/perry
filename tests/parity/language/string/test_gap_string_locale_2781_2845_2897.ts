// Gap test for #2781 (locale casing + localeCompare locale validation) and
// #2897 (legacy String.prototype.substr semantics). Compared byte-for-byte
// against `node --experimental-strip-types`.

// ---- #2897: String.prototype.substr (typed path) ----
const s = "abcdef";
console.log(s.substr(2, 3)); // "cde"
console.log(s.substr(-2, 1)); // "e"
console.log(s.substr(-2)); // "ef"
console.log(s.substr(2, -1)); // ""
console.log(s.substr(0)); // "abcdef"
console.log(s.substr(10, 2)); // ""
console.log(typeof String.prototype.substr); // "function"

// ---- #2897: substr on a dynamic / any-typed receiver ----
const dyn: any = "hello world";
console.log(dyn.substr(6, 3)); // "wor"
console.log(dyn.substr(-5)); // "world"

// ---- #5347: substr argument ToIntegerOrInfinity coercion (test262
// annexB/built-ins/String/prototype/substr tail) ----
const t: any = "abc";
console.log(t.substr(0, false)); // "" (false -> 0)
console.log(t.substr(1, NaN)); // "" (NaN -> 0)
console.log(t.substr(0, "")); // "" ("" -> 0)
console.log(t.substr(0, null)); // "" (null -> 0)
console.log(t.substr(2, undefined)); // "c" (undefined length -> rest)
console.log(t.substr(0, Infinity)); // "abc" (+Infinity length -> size)
console.log(t.substr(0, -Infinity)); // "" (-Infinity length -> 0, not "omitted")
console.log(t.substr(-Infinity)); // "abc" (-Infinity start -> 0)
console.log(t.substr("1", "1")); // "b" (numeric-string coercion)
console.log(t.substr(0, { valueOf: () => 2 })); // "ab" (object valueOf)

// ---- #2781: Turkic dotted/dotless I casing ----
console.log("I".toLocaleLowerCase("tr")); // "ı"
console.log("I".toLocaleLowerCase("en")); // "i"
console.log("i".toLocaleUpperCase("tr")); // "İ"
console.log("i".toLocaleUpperCase("en")); // "I"
console.log("I".toLocaleLowerCase("az")); // "ı"
console.log("I".toLocaleLowerCase()); // "i"
console.log("HELLO".toLocaleLowerCase("en-US")); // "hello"
console.log("hello".toLocaleUpperCase(["en", "tr"])); // "HELLO"
console.log(typeof String.prototype.toLocaleLowerCase); // "function"

// ---- #2781: dynamic / any-typed locale casing ----
const ds: any = "I";
console.log(ds.toLocaleLowerCase("tr")); // "ı"

// ---- #2781: BCP 47 validation throws RangeError on invalid tags ----
function r(fn: () => unknown): string {
  try {
    fn();
    return "no throw";
  } catch (e: any) {
    return e.name + ":" + e.message;
  }
}
console.log(r(() => "x".toLocaleLowerCase("not_a_locale")));
console.log(r(() => "x".toLocaleUpperCase("bad_tag")));
console.log(r(() => "x".localeCompare("y", "not_a_locale")));
console.log(r(() => "x".localeCompare("y", "en-US")));

// ---- localeCompare neutral collation still works ----
console.log("apple".localeCompare("banana")); // -1
console.log("b".localeCompare("a")); // 1
