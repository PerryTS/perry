// N6: a module `const` whose every value is one integer Number is read as
// that Number (wpfacts `const_numbers`). The positive cases are flag and
// size constants computed by operators; the negative controls are bindings
// that are not one Number, and reads that run in the dead zone.
function name(e: any): string { return e && e.constructor ? e.constructor.name : String(e); }

// --- negative control: a hoisted function called before the declarators.
function flagsEarly(m: number): number { return (m & EP) | CASTLE | DOUBLE; }
try { console.log("early call", flagsEarly(1)); } catch (e) { console.log("early call", name(e), (e as any).message); }
// a closure created before the declarators, called before and after them.
const lateReader = () => EP + CASTLE;
try { console.log("early closure", lateReader()); } catch (e) { console.log("early closure", name(e)); }
const registry: Array<() => number> = [];
function register(f: () => number): void { registry.push(f); }
register(function readsSize() { return SIZE * 2; });
try { console.log("registered early", registry[0]()); } catch (e) { console.log("registered early", name(e)); }
class EarlyUser {
  static early = (() => { try { return MASK; } catch (e) { return name(e); } })();
  read(): number { return MASK ^ 1; }
}
console.log("static early", EarlyUser.early);
try { console.log("method early", new EarlyUser().read()); } catch (e) { console.log("method early", name(e)); }

// --- the constants.
const EP = 1 << 17;
const CASTLE = 1 << 18;
const DOUBLE = 1 << 19;
const ALL = EP | CASTLE | DOUBLE;
const MASK = (1 << 7) - 1;
const SIZE = 8 * 8;
const NEG = -(1 << 20);
const INT_MIN = 1 << 31;
const WRAP = 1 << 32;
const U32 = -1 >>> 0;
const XOR = 0x55 ^ 0xff;
const AND = 0x1ff & ~0xf;
const SHIFTED = SIZE << 2;
export const EXPORTED = 1 << 9;

// --- negative controls: not one integer Number.
const NEG_ZERO = 0 * -1;
const HALF = 1 / 2;
const NAN = 0 / 0;
const TEXT = "t" + 1;
const TRUTH = EP > 0;
const BIG = 1n << 17n;
function pick(n: number): number { return n > 1000 ? 1 << 3 : 1 << 4; }
const EITHER = pick(process.argv.length);
let counter = 1 << 2;
var legacy = 1 << 5;
const OBJ = { v: 1 << 6 };

// --- readers: functions, methods, closures, initializers.
function make(from: number, to: number, flags: number): number { return from | (to << 7) | flags; }
function isEp(move: number): boolean { return (move & EP) !== 0; }
function kind(move: number): string {
  switch (move & ALL) {
    case EP: return "ep";
    case CASTLE: return "castle";
    case DOUBLE: return "double";
    default: return "quiet";
  }
}
function shadowed(EP: number): number { return EP + 1; }
function local(): number { const CASTLE = 5; return CASTLE + EP; }
function withDefault(m: number = DOUBLE): number { return m >> 17; }
function bump(): number { counter += 1; return counter; }
function others(): string {
  return [Object.is(NEG_ZERO, -0), HALF, NAN, TEXT, TRUTH, typeof BIG, EITHER, legacy, OBJ.v].join(",");
}
class Board {
  cells: number[] = new Array<number>(SIZE).fill(0);
  limit = SIZE - 1;
  static readonly WIDTH = SIZE >> 3;
  set(i: number, v: number): void { this.cells[i & MASK & this.limit] = v; }
  sum(): number { let s = 0; for (let i = 0; i < SIZE; i++) s += this.cells[i]; return s; }
  get flags(): number { return ALL; }
}
const table = [EP, CASTLE, DOUBLE].map((f) => (f & ALL) === f);
const reader = () => [NEG, INT_MIN, WRAP, U32, XOR, AND, SHIFTED, EXPORTED];

const moves = [make(1, 2, 0), make(3, 4, EP), make(5, 6, CASTLE), make(7, 8, DOUBLE), make(9, 10, EP | DOUBLE)];
for (const m of moves) console.log(m, isEp(m), kind(m), (m & CASTLE) !== 0 ? "c" : "-", m & MASK, (m >> 7) & MASK);
console.log("flags", EP, CASTLE, DOUBLE, ALL, MASK, SIZE);
console.log("edges", reader().join(" "));
console.log("typeof", typeof EP, EP === 131072, Object.is(EP, 131072), EP / 3, EP % 1000, -EP, ~EP, EP ** 2);
console.log("string", "" + EP, `${CASTLE}`, String(DOUBLE), EP.toString(16), JSON.stringify({ EP, ALL }));
console.log("float", EP + 0.5, EP * 1e300 * 1e300, EP / 0, Math.sqrt(SIZE), (NEG >>> 0), NEG >> 3);
console.log("shadow", shadowed(1), local(), withDefault(), withDefault(EP));
console.log("late", flagsEarly(EP), lateReader(), registry[0](), new EarlyUser().read());
console.log("others", others());
console.log("mutable", bump(), bump(), counter, legacy);
const b = new Board();
for (let i = 0; i < 200; i += 7) b.set(i, i);
console.log("board", b.sum(), b.limit, Board.WIDTH, b.flags, b.cells.length, table.join(","));
legacy = 7;
console.log("var", others());
