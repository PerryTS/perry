// #7837 — an erased `string` annotation is not a proof that the slot holds a
// string, so it may not choose the `+` OPERATOR.
//
// Two silent wrong answers came out of the same premise. `const s: string =
// (42 as any); s + 7` printed "427" (concat chosen where the spec adds), and
// `const t: string = (99 as any); t + "x"` printed "x" (the operand was
// decoded as the empty string and vanished).
//
// Read this file as two halves. The `lie:` rows are the bug. The `honest:`
// rows are the reason the fix cannot simply route everything through the
// dynamic helper: a real string operand must still concatenate, and a real
// number pair must still add. A fix that coerced everything to a string would
// pass the first half and fail the second.

const lie: any = 42;
const lie99: any = 99;

// ---- lie: the declared local, one-sided (`string` + non-string) ----
const a1: string = lie;
console.log("lie one-sided L", a1 + 7);
console.log("lie one-sided R", 7 + a1);
console.log("lie one-sided bool", a1 + true);
console.log("lie one-sided null", a1 + null);
console.log("lie one-sided undef", a1 + undefined);

// ---- lie: the declared local, both operands (`string` + `string`) ----
const a3: string = lie99;
console.log("lie pairwise L", a3 + "x");
console.log("lie pairwise R", "x" + a3);
const b1: string = lie;
const b2: string = lie99;
console.log("lie pairwise both", b1 + b2);

// ---- lie: the N-way chain fold ----
// `js_string_concat_chain` formats every part as a string, so it reproduces
// the source tree only when the FIRST node really concatenates.
console.log("lie chain tail-lit", b1 + b2 + "x");
console.log("lie chain no-lit", b1 + b2 + b1);
console.log("lie chain head-lit", "x" + b1 + b2);
console.log("lie chain mid-lit", b1 + "," + b2);

// ---- lie: reached through an alias and through a ternary ----
const e0: string = lie;
const e1 = e0;
console.log("lie alias", e1 + 7);
const i0: string = lie;
console.log("lie ternary", (true ? i0 : "q") + 7);

// ---- lie: a receiver-blind method-name guess ----
// `.slice(...)` is matched on the NAME, with no look at the receiver, so an
// array's `slice` claimed a string result and the operand disappeared.
const k0: any = [1, 2];
console.log("lie method-name", k0.slice(0) + 7);

// ---- lie: a declared `string` PARAMETER, kept away from the inliner ----
// Calling `pf` directly gets it inlined, which erases the parameter's declared
// type and hides the defect — that is why the first triage of this bug
// concluded parameters were safe. Through a function value it survives.
function pf(a: string, b: number): any {
  return a + b;
}
function pfr(a: string): any {
  return a + "x";
}
const viaValue: Array<(a: any, b: any) => any> = [pf as any, pfr as any];
console.log("lie param pair", viaValue[0](lie, 7));
console.log("lie param right", viaValue[1](lie99, 0));

// ---- honest: the declared local really holds a string ----
const h1: string = "ab";
const h2: number = 5;
console.log("honest local+num", h1 + h2);
console.log("honest num+local", h2 + h1);
console.log("honest local+lit", h1 + "cd");
console.log("honest chain", h1 + h2 + "z");
console.log("honest chain nolit", h1 + h1 + h1);
let acc: string = "";
for (let i = 0; i < 4; i++) {
  acc = acc + i;
}
console.log("honest build", acc);
console.log("honest method", h1.toUpperCase() + h2);

// ---- honest: arithmetic must stay arithmetic ----
const n1: number = 6;
const n2: number = 7;
console.log("honest add", n1 + n2);
console.log("honest add lit", n1 + 1);
console.log("honest typeof", typeof (n1 + n2), typeof (h1 + h2));

// ---- controls: the three shapes that were ALREADY correct ----
// A declared field, an object property and a `(string, number)` parameter pair
// route elsewhere; they must not acquire the defect from this fix.
interface Rec {
  t: string;
  n: number;
}
const rLie: Rec = { t: lie as any, n: 1 };
const rOk: Rec = { t: "q", n: 1 };
console.log("control field lie", rLie.t + 7);
console.log("control field ok", rOk.t + 7);
const anyObj: any = { t: 42 };
console.log("control prop lie", anyObj.t + 7);
class K {
  t: string;
  constructor(v: any) {
    this.t = v;
  }
  m(): any {
    return this.t + 7;
  }
}
console.log("control classfield lie", new K(lie).m());
console.log("control classfield ok", new K("q").m());
const arrLie: string[] = [lie as any];
const arrOk: string[] = ["q"];
console.log("control elem lie", arrLie[0] + 7);
console.log("control elem ok", arrOk[0] + 7);
console.log("control param direct", pf("q", 9));

// ---- controls: proven-string producers keep concatenating ----
console.log("proven String()", String(41) + 8);
console.log("proven stringify", JSON.stringify(41) + 8);
console.log("proven typeof", typeof lie + 8);
console.log("proven charcode", String.fromCharCode(65) + 8);
console.log("proven join", [1, 2].join("-") + 8);
console.log("proven method", "ab".slice(0) + 8);
