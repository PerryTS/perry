// A `+` whose operands are calls: the whole-program facts say what each call
// returns, and a pair of string calls is a concatenation. Every line must
// match node; the controls are calls that are no string calls.

function squareName(square: number): string {
  return "abcdefgh".charAt(square & 7) + String((square >> 4) + 1);
}
function promotionName(piece: number): "q" | "r" | "b" | "n" | null {
  if (piece === 5) return "q";
  if (piece === 4) return "r";
  if (piece === 3) return "b";
  if (piece === 2) return "n";
  return null;
}
function moveUci(move: number): string {
  return squareName(move & 127) + squareName((move >> 7) & 127) + (promotionName((move >> 14) & 7) ?? "");
}
function maybeUndefined(i: number): string | undefined {
  return i % 2 === 0 ? "even" : undefined;
}
function stringOrNumber(i: number): string | number {
  return i % 3 === 0 ? "s" + i : i;
}
function numberOnly(i: number): number {
  return i * 1.5;
}
function objectResult(i: number): { toString(): string } {
  return { toString() { return "obj" + i; } };
}
function bigResult(i: number): bigint {
  return BigInt(i);
}
function longName(i: number): string {
  return "a rather long string number " + i;
}
function uni(i: number): string {
  return i % 2 === 0 ? "é" : "\ud83d";
}
function uniLow(i: number): string {
  return i % 2 === 0 ? "€" : "\ude00";
}
class Namer {
  prefix: string;
  constructor(prefix: string) {
    this.prefix = prefix;
  }
  name(i: number): string {
    return this.prefix + String(i);
  }
  maybe(i: number): string | null {
    return i > 2 ? this.prefix : null;
  }
}
function codes(s: string): string {
  const out: number[] = [];
  for (let i = 0; i < s.length; i++) out.push(s.charCodeAt(i));
  return s.length + "[" + out.join(",") + "]";
}
function show(v: unknown): string {
  return typeof v + ":" + String(v);
}

const RUN = Math.random() < 2 ? 1 : 0;

// The shape: every from/to/promotion.
let acc = 0;
const seen: string[] = [];
for (let from = 0; from < 128 * RUN; from += 5) {
  if ((from & 0x88) !== 0) continue;
  for (let to = 0; to < 128; to += 7) {
    if ((to & 0x88) !== 0) continue;
    for (let p = 0; p < 8; p++) {
      const uci = moveUci(from | (to << 7) | (p << 14));
      acc += uci.length + uci.charCodeAt(uci.length - 1);
      if ((from + to + p) % 41 === 0) seen.push(uci);
    }
  }
}
console.log(acc, seen.join(" "));

// Calls on both sides.
for (let i = 0; i < 6 * RUN; i++) {
  console.log(
    codes(squareName(i) + squareName(i + 16)),
    codes(squareName(i) + longName(i)),
    codes(longName(i) + squareName(i)),
    codes(uni(i) + uniLow(i)),
    codes(uni(i) + squareName(i)),
    (uni(i) + uniLow(i)).codePointAt(0),
  );
}

// `??` defaults.
for (let i = 0; i < 6 * RUN; i++) {
  console.log(
    codes(squareName(i) + (promotionName(i) ?? "")),
    codes((promotionName(i) ?? "") + squareName(i)),
    codes((promotionName(i) ?? "-") + (promotionName(i + 1) ?? "")),
    codes(squareName(i) + (maybeUndefined(i) ?? "")),
    codes((maybeUndefined(i) ?? "") + (maybeUndefined(i + 1) ?? "odd")),
    codes(squareName(i) + (promotionName(i) ?? squareName(i + 1))),
  );
}

// Calls that are no string calls: `+` keeps its own meaning.
for (let i = 0; i < 7 * RUN; i++) {
  const a: any = stringOrNumber(i);
  const b: any = stringOrNumber(i + 1);
  console.log(
    show(a + b),
    show((stringOrNumber(i) as any) + (stringOrNumber(i + 3) as any)),
    show((numberOnly(i) as any) + (numberOnly(i + 1) as any)),
    show(squareName(i) + (numberOnly(i) as any)),
    show((numberOnly(i) as any) + squareName(i)),
    show((objectResult(i) as any) + squareName(i)),
    show(squareName(i) + (objectResult(i) as any)),
    show((bigResult(i) as any) + (bigResult(i + 1) as any)),
    show(squareName(i) + (bigResult(i) as any)),
    show((promotionName(i) as any) + squareName(i)),
    show((maybeUndefined(i) as any) + squareName(i)),
    show((stringOrNumber(i) ?? "") + squareName(i)),
  );
}

// Methods.
const namer = new Namer("n");
for (let i = 0; i < 5 * RUN; i++) {
  console.log(
    codes(namer.name(i) + namer.name(i + 1)),
    codes(namer.name(i) + (namer.maybe(i) ?? "")),
    codes(namer.name(i) + squareName(i) + namer.name(i * 100)),
    show((namer.maybe(i) as any) + namer.name(i)),
  );
}

// A function value that is reassigned: the call returns what it holds now.
let pickName: (i: number) => any = squareName;
const texts: string[] = [];
for (let i = 0; i < 4 * RUN; i++) {
  texts.push(show(pickName(i) + pickName(i + 1)));
  if (i === 1) pickName = numberOnly;
}
console.log(texts.join(" "));

// Results kept across collections.
const kept: string[] = [];
let total = 0;
for (let round = 0; round < 300 * RUN; round++) {
  for (let sq = 0; sq < 64; sq++) {
    const m = (sq + (sq & 7) * 8) | (((sq * 5) & 0x77) << 7) | ((round % 8) << 14);
    const uci = moveUci(m);
    const two = longName(round) + moveUci(m);
    total += uci.length + two.length;
    if ((sq * 31 + round) % 211 === 0) kept.push(uci + ":" + two);
  }
}
console.log(total, kept.length, kept[0], kept[kept.length - 1]);
