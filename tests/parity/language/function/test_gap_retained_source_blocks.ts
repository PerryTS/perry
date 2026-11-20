// Exact source text: whitespace, Unicode, escapes and nested byte ranges.
function café(α = "世界😀é") {
  /* retained café / 世界 / 😀 */
  function intérieur(x = "\ud800\0") { return x + α; }
  return intérieur;
}
const inner = café();
const arrow = (value = "λ") => /* exact arrow */ value + "😀";
async function later(value = "é") { return value; }
function* sequence(value = "🌍") { yield value; }
async function* laterSequence(value = "世界") { yield value; }
class Étage {
  constructor(value = "café") { this.value = value; }
  méthode(x = "😀") { return x + this.value; }
  static flèche = (x = "λ") => x;
}
const Anonyme = class { nested() { return () => "世界"; } };
for (const fn of [café, inner, arrow, later, sequence, laterSequence,
                  Étage, Étage.prototype.méthode, Étage.flèche, Anonyme]) {
  console.log(JSON.stringify(Function.prototype.toString.call(fn)));
  console.log(JSON.stringify(String(fn)));
}

// A function used as a property key needs the complete exact source too.
const sourceKeys: any = {};
sourceKeys[café as any] = "retained source key";
console.log(sourceKeys[String(café)]);

// Raw cutoff (own source <= 512 bytes raw, longer in frames): short and long
// functions of every kind, short ones nested in long ones and the reverse.
const filler = (n: number) => "/* " + "世界😀é".repeat(n) + " */";
function longOuter(a = "é") {
  /* LONG-OUTER ============================================================
     ====================================================================== */
  function shortInside(x = "世界") { return x + a; }
  const arrowInside = (y = 1) => /* tiny */ y;
  async function* longAsyncGen(z = "λ") {
    /* =======================================================================================================================================================================================================================================================================================================================================
       =================================================================== */
    yield shortInside(z);
    yield (() => z)();
    yield [1, 2, 3].map((q) => q * 2).join("+") + "=========================";
  }
  return { shortInside, arrowInside, longAsyncGen };
}
const parts = longOuter();
const longArrow = (value = "λ") => {
  /* ================================================================================================================================================================================================================================================================================================================================================
     ============================================================================
     ============================================================================ */
  return value + "😀";
};
class LongClass {
  /* ========================================================================================================================================================================================================================================================================================================================================= */
  short(x = 1) { return x; }
  longMethod(y = "世界") {
    /* ==========================================================================================================================================================================================================================================================================================================================================
       ====================================================================== */
    return y;
  }
}
for (const fn of [longOuter, parts.shortInside, parts.arrowInside, parts.longAsyncGen,
                  longArrow, LongClass, LongClass.prototype.short,
                  LongClass.prototype.longMethod]) {
  const text = Function.prototype.toString.call(fn);
  console.log(text.length, JSON.stringify(text));
  console.log(String(fn) === text);
}
// Function-valued property keys, short and long.
const keyed: any = {};
keyed[parts.shortInside as any] = 1;
keyed[longOuter as any] = 2;
console.log(keyed[String(parts.shortInside)], keyed[String(longOuter)], Object.keys(keyed).length);
console.log(filler(1).length);
