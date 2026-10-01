// #11558: reading an ordinary property off a registered symbol must answer
// `undefined`, whatever memory happens to sit in front of the symbol.
//
// `Symbol.for` and well-known symbols are Box-leaked `SymbolHeader`s with no
// `GcHeader`. Under mimalloc the word in front of one is the tail of the
// previous block, and for a symbol allocated right after another symbol that
// word is the previous symbol's `id`. Header-trusting classifiers read it as
// `obj_type`: id 17 is `GC_TYPE_DATE_CELL`, id 18 `GC_TYPE_TEMPORAL`. The next
// symbol then took the Date (or Temporal) arm of the generic property read,
// whose own-property probe dereferenced the symbol's magic word as the cell's
// metadata pointer. claude-code 2.1.112's `doctor` SIGSEGV'd there: React reads
// `type.defaultProps` and an element type can be a `Symbol.for(...)` symbol.
//
// The test lays that memory out the way cc's startup did. `Symbol.for(k)` on
// an already-registered key copies `k` into a temporary Rust `String` and drops
// it; the next NEW symbol's `SymbolHeader` is then carved from the very next
// mimalloc block of the same (32-byte) size class. With a 32-byte `k` whose
// last eight bytes are 0x11 0 0 0 0 0 0 0, the word in front of every new
// symbol reads as `GC_TYPE_DATE_CELL`. An accessor is installed first because
// the crashing probe only runs once descriptors are in use, and the reads go
// through shared, IC-warmed sites.
//
// Output must be byte-identical to node.

const holder: any = {};
Object.defineProperty(holder, "x", {
  get() {
    return 1;
  },
});

var symbols: symbol[];
var readDefaultProps: (t: any) => unknown;
var readByKey: (t: any, k: string) => unknown;
const init = () => {
  // 24 ASCII bytes, then the little-endian word 17 (GC_TYPE_DATE_CELL).
  const spacer = "abcdefghijklmnopqrstuvwx\u0011\u0000\u0000\u0000\u0000\u0000\u0000\u0000";
  Symbol.for(spacer);
  symbols = [];
  for (let i = 0; i < 64; i++) {
    Symbol.for(spacer);
    symbols.push(Symbol.for("perry.11558." + i));
  }
  readDefaultProps = (t) => t.defaultProps;
  readByKey = (t, k) => t[k];
};
init();

readDefaultProps({ defaultProps: 1 });
readDefaultProps({ a: 1, defaultProps: 2 });
readDefaultProps(function f() {});
readByKey({ k: 1 }, "k");
readByKey([1, 2], "length");

let undefinedReads = 0;
let otherReads = 0;
for (const s of symbols) {
  if (readDefaultProps(s) === undefined) undefinedReads++;
  else otherReads++;
  if (readByKey(s, "defaultProps") === undefined) undefinedReads++;
  else otherReads++;
}
console.log("symbols: " + symbols.length);
console.log("undefined reads: " + undefinedReads);
console.log("other reads: " + otherReads);
console.log("description: " + symbols[17].description);
console.log("same symbol: " + (Symbol.for("perry.11558.17") === symbols[17]));
console.log("accessor: " + holder.x);
