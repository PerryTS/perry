// A `class X extends Map|Set` override must win for calls through an untyped
// receiver, generic or not, exactly as it does through a typed one.
class Counting<K, V> extends Map<K, V> {
  hits = 0;
  get(k: K) {
    this.hits++;
    return super.get(k);
  }
}
class Plain extends Map<string, number> {
  get(_k: string) {
    return -1;
  }
}
class Tagged<T> extends Set<T> {
  has(v: T) {
    return v === ("tag" as any) ? true : super.has(v);
  }
}
const call = (m: any, k: string) => m.get(k);
const c = new Counting<string, number>([["a", 1]]);
console.log(call(c, "a"), call(c, "b"), c.get("a"), c.hits);
const p = new Plain([["a", 1]]);
console.log(call(p, "a"), p.get("a"), Map.prototype.get.call(p, "a"));
const t = new Tagged<string>(["x"]);
const has = (s: any, v: string) => s.has(v);
console.log(has(t, "tag"), has(t, "x"), has(t, "y"));
const plain = new Map([["k", 2]]);
console.log(call(plain, "k"), call(new Map(), "k"));
