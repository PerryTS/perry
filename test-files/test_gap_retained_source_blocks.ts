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
