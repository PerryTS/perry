// Strict equality against a string literal is lowered inline (no js_eq call).
// The inline sequence decides by NaN-box tag, by the pooled literal's pointer,
// by the literal's compile-time SSO immediate, and by byte_len + first/last
// byte — so every one of those shortcuts needs an ECMAScript edge pinned here.
//
// The interesting inputs are the ones that are NOT the pooled literal pointer:
// concatenations, substrings, charAt (which yields an inline SSO value),
// JSON.parse, String.fromCharCode. Those are the representations the fast path
// has to reconcile with a heap literal.

function anyv(x: any): any {
  return x;
}

// ---- same content, different representation --------------------------------
const heapNum: any = anyv("nu" + "m"); // runtime-built heap string
const subNum: any = anyv("xnumx".substring(1, 4));
const jsonNum: any = anyv(JSON.parse('"num"'));
const codeNum: any = anyv(String.fromCharCode(110, 117, 109));
console.log(heapNum === "num", subNum === "num", jsonNum === "num", codeNum === "num");
console.log("num" === heapNum, heapNum !== "num");

// charAt yields a 1-byte value; "+" is a 1-byte literal.
const plus: any = anyv("a+b".charAt(1));
const minus: any = anyv("a-b".charAt(1));
console.log(plus === "+", minus === "+", plus === "-", plus !== "+");

// ---- same length, differing first / last / middle byte ---------------------
const bin: any = anyv("bi" + "n");
console.log(bin === "num", bin === "bin", bin === "bit");
const long1: any = anyv("abcd" + "efg");
console.log(long1 === "abcdefg", long1 === "abcXefg", long1 === "abcdefX", long1 === "Xbcdefg");
console.log(long1 === "abcdef", long1 === "abcdefgh");

// ---- the empty string ------------------------------------------------------
const empty: any = anyv("ab".substring(0, 0));
console.log(empty === "", empty === "a", "" === empty, empty !== "");

// ---- NaN and signed zero ---------------------------------------------------
const nan: any = anyv(NaN);
const negZero: any = anyv(-0);
console.log(nan === nan, nan !== nan, NaN === NaN);
console.log(negZero === 0, 0 === negZero, negZero === -0, Object.is(negZero, -0));
console.log(Math.sqrt(-1) === Math.sqrt(-1));

// ---- typed vs boxed numeric representations --------------------------------
// (3 | 0) is an int32-tagged value; 3.0 is a plain double. Same Number.
const asInt: any = anyv(3 | 0);
const asDouble: any = anyv(3.0);
const fromArray: any = anyv([3][0]);
console.log(asInt === asDouble, asDouble === asInt, asInt === fromArray, asInt === 3);
console.log(asInt === "3", "3" === asInt, asInt === 4);

// ---- cross-type: nothing non-string is === a string ------------------------
console.log(
  (5 as any) === "5",
  (true as any) === "true",
  (null as any) === "null",
  (undefined as any) === "undefined",
);
console.log(null === undefined, (null as any) !== undefined, undefined === undefined);
const boxed: any = anyv(new String("num"));
console.log(boxed === "num", String("num") === "num", boxed == "num");
const sym: any = anyv(Symbol("num"));
console.log(sym === "num", sym === sym);
const big: any = anyv(BigInt(3));
console.log(big === "3", big === 3, big === BigInt(3));

// ---- object identity -------------------------------------------------------
const o1: any = anyv({ kind: "num" });
const o2: any = anyv({ kind: "num" });
console.log(o1 === o1, o1 === o2, o1 === "num", o1.kind === "num", o2.kind === o1.kind);
const arr: any = anyv([1, 2]);
console.log(arr === arr, arr === "1,2");

// ---- multi-byte UTF-8: first/last BYTE, not first/last character -----------
const acc: any = anyv("é" + "");
console.log(acc === "é", acc === "è", acc === "e");
const emoji: any = anyv("\u{1F600}" + "");
console.log(emoji === "\u{1F600}", emoji === "\u{1F601}");
const cjk: any = anyv("日本" + "");
console.log(cjk === "日本", cjk === "日月", cjk === "月本");

// ---- the interpreter's own shape: union tag dispatch -----------------------
type Node =
  | { kind: "num"; num: number }
  | { kind: "str"; str: string }
  | { kind: "bin"; op: string; left: Node; right: Node };

function describe(n: Node): string {
  if (n.kind === "num") return "N" + n.num;
  if (n.kind === "str") return "S" + n.str;
  if (n.op === "+") return "(" + describe(n.left) + "+" + describe(n.right) + ")";
  if (n.op === "*") return "(" + describe(n.left) + "*" + describe(n.right) + ")";
  return "?";
}

const tree: Node = {
  kind: "bin",
  op: "+",
  left: { kind: "num", num: 1 },
  right: { kind: "bin", op: "*", left: { kind: "str", str: "x" }, right: { kind: "num", num: 2 } },
};
console.log(describe(tree));

// The op strings here are built at runtime, so they are NOT the pooled literal.
const dynOp: string = ["+", "-", "*"][1];
console.log(dynOp === "-", dynOp === "+", dynOp !== "-");

// ---- non-literal heap strings (the environment-lookup shape from #8591) ---
// Keep both operands statically `string` but construct distinct heap values so
// the no-literal equality path has to compare their contents. Lengths 0..3 are
// settled inline; longer strings retain the runtime helper.
function sameString(a: string, b: string): boolean {
  return a === b;
}
const heapEmptyA: string = "xy".substring(1, 1);
const heapEmptyB: string = "ab".substring(0, 0);
const heapOneA: string = "xn".substring(1);
const heapOneB: string = "ny".substring(0, 1);
const heapTwoA: string = "g" + "o";
const heapTwoB: string = "xgoy".substring(1, 3);
const heapThreeA: string = "f" + "ib";
const heapThreeB: string = "xfiby".substring(1, 4);
const heapLongA: string = "long" + "name";
const heapLongB: string = "xlongnamey".substring(1, 9);
console.log(
  sameString(heapEmptyA, heapEmptyB),
  sameString(heapOneA, heapOneB),
  sameString(heapTwoA, heapTwoB),
  sameString(heapThreeA, heapThreeB),
  sameString(heapLongA, heapLongB),
);
console.log(
  sameString(heapOneA, "z".substring(0, 1)),
  sameString(heapTwoA, "no".substring(0, 2)),
  sameString(heapThreeA, "fox".substring(0, 3)),
  sameString(heapThreeA, "fig".substring(0, 3)),
  sameString(heapLongA, "longnames".substring(0, 9)),
);

// ---- switch/case over the same literals (a second lowering of ===) ---------
function classify(s: string): number {
  switch (s) {
    case "num":
      return 1;
    case "str":
      return 2;
    case "bin":
      return 3;
    default:
      return 0;
  }
}
console.log(classify(heapNum), classify("str"), classify(subNum), classify("zzz"));

// ---- literal vs literal ----------------------------------------------------
console.log("num" === "num", "num" === "bin", "num" !== "bin", "if" === "if", "" === "");
