// A `size` read on a Map/Set is the ordinary [[Get]]: a subclass accessor,
// a redefined Map.prototype.size, or a relinked prototype answers, not the
// storage count. A plain Map/Set still answers its entry count.
// DEC 250: Set.prototype.size is locked in a compiled program, so the redefinition is refused and size reads the builtin accessor.
// tests/parity/expected holds Perry's output.
function locked(f: () => void) { try { f(); } catch (e) { console.log("DEC 250: " + (e as Error).message); } }
class CountingMap<K, V> extends Map<K, V> {
  get size(): number {
    return super.size + 100;
  }
}
class TaggedSet<T> extends Set<T> {
  get size(): number {
    return super.size * 10;
  }
}
function readSize(x: any): any {
  return x.size;
}
const cm = new CountingMap<number, number>();
cm.set(1, 1);
console.log("subclass map", cm.size, readSize(cm));
const ts = new TaggedSet<string>();
ts.add("a");
ts.add("b");
console.log("subclass set", ts.size, readSize(ts));
const plain = new Map<string, number>([["a", 1], ["b", 2], ["c", 3]]);
console.log("plain", plain.size, readSize(plain), readSize(new Set([1])));
const relinked: any = new Map([[1, 2]]);
Object.setPrototypeOf(relinked, { get size() { return "proto"; } });
console.log("relinked", readSize(relinked));
locked(() => { Object.defineProperty(Set.prototype, "size", { get() { return -1; }, configurable: true }); });
console.log("redefined set", readSize(new Set([1, 2])), readSize(plain));
