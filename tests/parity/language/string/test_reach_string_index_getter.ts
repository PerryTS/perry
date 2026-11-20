// An index getter a program installs on String.prototype runs when a string
// index read goes past the end: the miss continues on the chain. The
// whole-program solve walks no accessor for a primitive receiver, so the
// getter's body was called inactive although the reads below enter it
// (check mode: "inactive body: body entered"; `PERRY_FACTS_DCE=1` trapped in
// the removed body; `PERRY_FACTS_REACH_PROTO_ACCESSORS=0` restores that).
function typed(t: string, k: number) { return t[k]; }
function untyped(t: any, k: number) { return t[k]; }
let calls = 0;
Object.defineProperty(String.prototype, "5", {
  get(this: any) { calls++; return "<" + this.length + ">"; }, configurable: true,
});
let out = "";
for (let k = 0; k < 7; k++) out += typed("abc", k) + untyped("xy", k) + ";";
console.log(out);
// Negative controls: an index inside the string never reaches the chain; an
// index no getter names reads undefined.
console.log(typed("abcdefg", 5), untyped("abcdefg", 5), typed("abc", 6), untyped("abc", 4), calls);
