// A tree-walking interpreter for a small functional language.
// Written to stay inside scriptc's STATIC tier (no generics, no `any`,
// no `var`/`==`, dense arrays, string-keyed Maps, discriminated unions).
// Exercises: recursive union allocation, closures capturing environments,
// polymorphic dispatch, string building, deep recursion, Map lookups.

type Node =
  | { kind: "num"; num: number }
  | { kind: "str"; str: string }
  | { kind: "var"; name: string }
  | { kind: "bin"; op: string; left: Node; right: Node }
  | { kind: "if"; cond: Node; then: Node; alt: Node }
  | { kind: "let"; name: string; value: Node; body: Node }
  | { kind: "fun"; param: string; body: Node }
  | { kind: "call"; target: Node; arg: Node };

type Env = { names: string[]; vals: Value[]; parent: Env | null };

type Value =
  | { kind: "num"; num: number }
  | { kind: "str"; str: string }
  | { kind: "clo"; param: string; body: Node; env: Env };

// ---------- lexer ----------

type Token = { kind: string; text: string };

function isDigit(c: string): boolean {
  return c >= "0" && c <= "9";
}

function isIdentStart(c: string): boolean {
  return (c >= "a" && c <= "z") || (c >= "A" && c <= "Z") || c === "_";
}

function isIdentPart(c: string): boolean {
  return isIdentStart(c) || isDigit(c);
}

function lex(src: string): Token[] {
  const out: Token[] = [];
  let i = 0;
  while (i < src.length) {
    const c = src.charAt(i);
    if (c === " " || c === "\n" || c === "\t") {
      i = i + 1;
      continue;
    }
    if (isDigit(c)) {
      let j = i;
      while (j < src.length && isDigit(src.charAt(j))) j = j + 1;
      out.push({ kind: "num", text: src.substring(i, j) });
      i = j;
      continue;
    }
    if (isIdentStart(c)) {
      let j = i;
      while (j < src.length && isIdentPart(src.charAt(j))) j = j + 1;
      const word = src.substring(i, j);
      if (word === "let" || word === "in" || word === "if" || word === "then" || word === "else" || word === "fun") {
        out.push({ kind: word, text: word });
      } else {
        out.push({ kind: "ident", text: word });
      }
      i = j;
      continue;
    }
    if (c === '"') {
      let j = i + 1;
      while (j < src.length && src.charAt(j) !== '"') j = j + 1;
      out.push({ kind: "str", text: src.substring(i + 1, j) });
      i = j + 1;
      continue;
    }
    if (c === "<" && i + 1 < src.length && src.charAt(i + 1) === "=") {
      out.push({ kind: "op", text: "<=" });
      i = i + 2;
      continue;
    }
    out.push({ kind: c === "(" || c === ")" ? c : "op", text: c });
    i = i + 1;
  }
  out.push({ kind: "eof", text: "" });
  return out;
}

// ---------- parser (precedence climbing) ----------

type Parser = { toks: Token[]; pos: number };

function peek(p: Parser): Token {
  return p.toks[p.pos];
}

function advance(p: Parser): Token {
  const t = p.toks[p.pos];
  p.pos = p.pos + 1;
  return t;
}

function expect(p: Parser, kind: string): void {
  const t = advance(p);
  if (t.kind !== kind) {
    console.log("parse error: wanted " + kind + " got " + t.kind);
  }
}

function precOf(op: string): number {
  if (op === "<" || op === ">" || op === "<=") return 1;
  if (op === "+" || op === "-") return 2;
  if (op === "*" || op === "/") return 3;
  return 0;
}

function parseAtom(p: Parser): Node {
  const t = peek(p);
  if (t.kind === "num") {
    advance(p);
    return { kind: "num", num: parseInt(t.text, 10) };
  }
  if (t.kind === "str") {
    advance(p);
    return { kind: "str", str: t.text };
  }
  if (t.kind === "ident") {
    advance(p);
    return { kind: "var", name: t.text };
  }
  if (t.kind === "(") {
    advance(p);
    const inner = parseExpr(p, 0);
    expect(p, ")");
    return inner;
  }
  if (t.kind === "fun") {
    advance(p);
    const name = advance(p);
    const body = parseExpr(p, 0);
    return { kind: "fun", param: name.text, body: body };
  }
  if (t.kind === "let") {
    advance(p);
    const name = advance(p);
    expect(p, "op"); // '='
    const value = parseExpr(p, 0);
    expect(p, "in");
    const body = parseExpr(p, 0);
    return { kind: "let", name: name.text, value: value, body: body };
  }
  if (t.kind === "if") {
    advance(p);
    const cond = parseExpr(p, 0);
    expect(p, "then");
    const then = parseExpr(p, 0);
    expect(p, "else");
    const alt = parseExpr(p, 0);
    return { kind: "if", cond: cond, then: then, alt: alt };
  }
  advance(p);
  return { kind: "num", num: 0 };
}

function parseApply(p: Parser): Node {
  let head = parseAtom(p);
  while (true) {
    const t = peek(p);
    if (t.kind === "num" || t.kind === "str" || t.kind === "ident" || t.kind === "(") {
      const arg = parseAtom(p);
      head = { kind: "call", target: head, arg: arg };
      continue;
    }
    return head;
  }
}

function parseExpr(p: Parser, minPrec: number): Node {
  let left = parseApply(p);
  while (true) {
    const t = peek(p);
    if (t.kind !== "op") return left;
    const prec = precOf(t.text);
    if (prec === 0 || prec < minPrec) return left;
    advance(p);
    const right = parseExpr(p, prec + 1);
    left = { kind: "bin", op: t.text, left: left, right: right };
  }
}

function parse(src: string): Node {
  const p: Parser = { toks: lex(src), pos: 0 };
  return parseExpr(p, 0);
}

// ---------- evaluator ----------

function lookup(env: Env, name: string): Value {
  let e: Env | null = env;
  while (e !== null) {
    const names = e.names;
    for (let i = 0; i < names.length; i++) {
      if (names[i] === name) return e.vals[i];
    }
    e = e.parent;
  }
  return { kind: "num", num: 0 };
}

function asNum(v: Value): number {
  if (v.kind === "num") return v.num;
  return 0;
}

function evalNode(n: Node, env: Env): Value {
  if (n.kind === "num") return { kind: "num", num: n.num };
  if (n.kind === "str") return { kind: "str", str: n.str };
  if (n.kind === "var") return lookup(env, n.name);
  if (n.kind === "fun") return { kind: "clo", param: n.param, body: n.body, env: env };
  if (n.kind === "bin") {
    const l = evalNode(n.left, env);
    const r = evalNode(n.right, env);
    if (n.op === "+" && l.kind === "str") {
      const rs = r.kind === "str" ? r.str : "" + asNum(r);
      return { kind: "str", str: l.str + rs };
    }
    const a = asNum(l);
    const b = asNum(r);
    if (n.op === "+") return { kind: "num", num: a + b };
    if (n.op === "-") return { kind: "num", num: a - b };
    if (n.op === "*") return { kind: "num", num: a * b };
    if (n.op === "/") return { kind: "num", num: a / b };
    if (n.op === "<") return { kind: "num", num: a < b ? 1 : 0 };
    if (n.op === ">") return { kind: "num", num: a > b ? 1 : 0 };
    if (n.op === "<=") return { kind: "num", num: a <= b ? 1 : 0 };
    return { kind: "num", num: 0 };
  }
  if (n.kind === "if") {
    const c = evalNode(n.cond, env);
    if (asNum(c) !== 0) return evalNode(n.then, env);
    return evalNode(n.alt, env);
  }
  if (n.kind === "let") {
    // Recursive let: bind the name first, then evaluate the value inside the
    // new scope and patch the slot. A closure defined here captures an
    // environment that contains the closure itself — a genuine reference
    // cycle, which is the point.
    const inner: Env = { names: [n.name], vals: [{ kind: "num", num: 0 }], parent: env };
    const v = evalNode(n.value, inner);
    inner.vals[0] = v;
    return evalNode(n.body, inner);
  }
  const fn = evalNode(n.target, env);
  const arg = evalNode(n.arg, env);
  if (fn.kind === "clo") {
    const inner: Env = { names: [fn.param], vals: [arg], parent: fn.env };
    return evalNode(fn.body, inner);
  }
  return { kind: "num", num: 0 };
}

// ---------- driver ----------

const FIB = "let fib = fun n if n <= 1 then n else fib (n - 1) + fib (n - 2) in fib 21";
const SUMLOOP = "let go = fun i if i <= 0 then 0 else i + go (i - 1) in go 250";
const STRWORK = 'let cat = fun i if i <= 0 then "" else cat (i - 1) + "x" in cat 400';

function main(): void {
  let checksum = 0;
  const progs: string[] = [FIB, SUMLOOP, STRWORK];
  for (let round = 0; round < 400; round++) {
    for (let k = 0; k < progs.length; k++) {
      const ast = parse(progs[k]);
      const env: Env = { names: [], vals: [], parent: null };
      const out = evalNode(ast, env);
      if (out.kind === "num") {
        checksum = checksum + out.num;
      } else if (out.kind === "str") {
        checksum = checksum + out.str.length;
      }
    }
  }
  console.log(checksum);
}
main();
