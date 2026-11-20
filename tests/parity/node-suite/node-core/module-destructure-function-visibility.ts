// Regression (#6649, pi wall #4): module-scope ARRAY-destructured bindings
// must be visible (with their real values) from function bodies, closures,
// and class methods — not just from top-level statements.
//
// The array-pattern lowering wraps each leaf `Stmt::Let` in the
// iterator-protocol `Stmt::Try` scaffolding (IteratorClose on abrupt
// completion). codegen's module-global promotion pre-walk only scanned
// TOP-LEVEL init statements for `Stmt::Let`, so array-destructured leaves
// were never globalized and every function reference compiled to the
// not-in-scope fallback (`undefined`). Object-pattern leaves (emitted at
// statement level) were unaffected — which is why the bug hid until pi:
// TypeBox's hash module does
//
//   var [Prime, Size] = [BigInt("1099511628211"), BigInt("18446744073709551616")];
//   function FNV1A64_OP(byte) { Accumulator = Accumulator * Prime % Size; }
//
// and `Accumulator * Prime` saw `ToNumeric(undefined) = NaN` → a spurious
// "Cannot mix BigInt and other types" TypeError during pi-native init while
// node printed pi's usage cleanly.

// --- the TypeBox FNV-1a shape, verbatim structure ---
var Accumulator = BigInt("14695981039346656037");
var [Prime, Size] = [BigInt("1099511628211"), BigInt("18446744073709551616")];
var Bytes = Array.from({ length: 256 }).map((_x, i) => BigInt(i));
function FNV1A64_OP(byte: number) {
  Accumulator = Accumulator ^ Bytes[byte];
  Accumulator = Accumulator * Prime % Size;
}
function HashCode() {
  Accumulator = BigInt("14695981039346656037");
  FNV1A64_OP(8);
  return Accumulator;
}
console.log("fnv:", HashCode().toString(16).padStart(16, "0"));
console.log("prime:", typeof Prime, String(Prime), "size:", typeof Size, String(Size));

// --- every destructuring form, every primitive family, read from a function ---
var [S1] = ["hello"];
var [N1] = [42.5];
var [B1] = [7n];
const [C1] = ["c-const"];
let [L1] = ["l-let"];
var [A1, [A2]] = ["a1", ["a2"]];
var [D1 = "dflt"] = [];
var [...R1] = ["r1", "r2"];
var [P1, ...P2] = ["p1", "p2", "p3"];
var { O1 } = { O1: "o-var" };
var [{ M1 }] = [{ M1: "mixed" }];
function readAll(): string {
  return [
    typeof S1, S1,
    typeof N1, N1,
    typeof B1, String(B1),
    typeof C1, C1,
    typeof L1, L1,
    A1, A2, D1,
    Array.isArray(R1) ? R1.join(",") : typeof R1,
    P1, Array.isArray(P2) ? P2.join(",") : typeof P2,
    O1, M1,
  ].join("|");
}
console.log("fn:", readAll());
const readArrow = () => `${S1}/${String(B1)}/${A2}/${D1}`;
console.log("arrow:", readArrow());
class Reader {
  read(): string {
    return `${N1}:${C1}:${P2.length}`;
  }
  static sread(): string {
    return `${L1}:${R1[1]}`;
  }
}
console.log("method:", new Reader().read());
console.log("static:", Reader.sread());
console.log("top:", S1, N1, String(B1), C1, L1, A1, A2, D1, R1.join(","), P1, P2.join(","), O1, M1);

// --- writes from a function flow back through the same binding ---
function bump() {
  L1 = L1 + "!";
  N1 = N1 + 1;
}
bump();
console.log("after-bump fn:", (() => `${L1} ${N1}`)());
console.log("after-bump top:", L1, N1);
