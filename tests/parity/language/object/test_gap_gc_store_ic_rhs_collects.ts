// The static-key store `o.k = v` has ONE inline path for ANY right-hand side
// (perry-codegen `expr/put_value_store_ic.rs`). An RHS may run a collection
// that MOVES the receiver: the reference `o` is evaluated first and held in an
// operand root across the RHS, and the receiver's shape is read from that root
// only AFTER the RHS returns. Here every RHS allocates in a loop (a moving
// safepoint under PERRY_GC_MOVING_LOOP_POLLS=1), the receivers are young, and
// each site is primed before the collections start, so the stores are inline
// HITS on a receiver the RHS just moved. A store through a register captured
// before the RHS writes the from-space copy and the value is lost.

function churn(n: number): { v: number; s: string } {
  let last = { v: -1, s: "" };
  for (let i = 0; i < n; i++) {
    last = { v: i, s: "x" + i };
  }
  return last;
}

// Receiver = a parameter: the dominant real-code form.
function putObj(o: any, n: number): void {
  o.k = churn(n);
}
function putNum(o: any, n: number): void {
  o.k = churn(n).v * 2;
}
function putStr(o: any, n: number): void {
  o.k = "s" + churn(n).s;
}
// Receivers that are COMPUTED, not a local: the reference is an element or a
// property read, evaluated once, before the RHS. Its value must survive the
// RHS's collection through the operand root, not through the local's slot.
function putElem(arr: any[], j: number, n: number): void {
  arr[j].k = churn(n);
}
function putDeep(h: any, n: number): void {
  h.inner.k = churn(n).v + 1;
}

function check(label: string, cond: boolean, o: any): boolean {
  if (!cond) console.log("BAD", label, JSON.stringify(o));
  return cond;
}

let good = 0;
const keep: any[] = [];
for (let r = 0; r < 120; r++) {
  // Literal receivers (an anon-shape class instance) and parsed receivers
  // (class-less, birth-marked ordinary): both kinds the inline path admits.
  const lit: any = { a: r, k: null, b: "q" + r };
  const parsed: any = JSON.parse('{"a":' + r + ',"k":null,"b":"p"}');
  const n = r < 3 ? 1 : 3000;
  putObj(lit, n);
  putObj(parsed, n);
  let ok = check("obj-lit", lit.k.v === n - 1 && lit.k.s === "x" + (n - 1) && lit.a === r, lit);
  ok = check("obj-parsed", parsed.k.v === n - 1 && parsed.a === r && parsed.b === "p", parsed) && ok;
  putNum(lit, n);
  ok = check("num", lit.k === (n - 1) * 2 && lit.b === "q" + r, lit) && ok;
  putStr(parsed, n);
  ok = check("str", parsed.k === "sx" + (n - 1), parsed) && ok;
  const arr: any[] = [null, { a: -r, k: null, b: "e" }];
  putElem(arr, 1, n);
  ok = check("elem", arr[1].k.v === n - 1 && arr[1].a === -r, arr[1]) && ok;
  const h: any = { inner: { a: r, k: 0, b: "d" } };
  putDeep(h, n);
  ok = check("deep", h.inner.k === n && h.inner.b === "d", h.inner) && ok;
  if (!ok) break;
  keep.push(lit, parsed);
  if (keep.length > 40) keep.splice(0, 2);
  good++;
}
console.log("rounds", good);
churn(200000);
let sum = 0;
let text = "";
for (const o of keep) {
  sum += o.a;
  text += typeof o.k === "string" ? o.k.length : o.k;
  text += ",";
}
console.log(sum, text.length, text.slice(0, 40));
