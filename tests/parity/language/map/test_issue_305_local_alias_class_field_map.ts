// Closes #305: `const m = this.map` local alias of a class-field Map<K,V>
// dropped the Generic{base="Map"} type because infer_type_from_expr's Member
// arm hit the catch-all _ => Type::Any. Result: `m.set(k, v)` and
// `for (const [k,v] of m)` both fell off the Map fast path — set wrote into
// the wrong slot (key showed as 0) and for-of ran 0 iterations. v0.5.388's
// #302 fix added the same registry lookup for the for-of source resolver but
// not for the Let-RHS type inference. Same one-line gap, different consumer.
//
// #305 round 2 (re-reported on v0.5.415): the initial fix only covered
// fields with EXPLICIT type annotations (`map: Map<number, string> = ...`).
// Fields with inferred-from-initializer types (`private map = new Map<K,V>()`,
// the more common shape in real code) still hit the Type::Any path because
// `lower_class_prop` only consulted `prop.type_ann`, dropping the
// `new Map<K,V>` generic shape on the floor. Both `for-of this.map` (direct)
// AND `for-of local = this.map` (alias) silently iterated 0 times.
// Fix: when the annotation is absent, fall back to `infer_type_from_expr`
// on the initializer — same routine that already resolves `let m = new Map<K,V>()`.

class Example {
  private map: Map<number, string> = new Map();

  runViaLocal(key: number): void {
    const m = this.map;
    m.set(key, "hello");

    const keys = Array.from(m.keys());
    let iterCount = 0;
    for (const [k, v] of m) {
      iterCount++;
    }

    console.log(`size: ${m.size}`);
    console.log(`keys: ${JSON.stringify(keys)}`);
    console.log(`iterCount: ${iterCount}`);
    this.map.clear();
  }

  runViaDirect(key: number): void {
    this.map.set(key, "hello");

    const keys = Array.from(this.map.keys());
    let iterCount = 0;
    for (const [k, v] of this.map) {
      iterCount++;
    }

    console.log(`size: ${this.map.size}`);
    console.log(`keys: ${JSON.stringify(keys)}`);
    console.log(`iterCount: ${iterCount}`);
    this.map.clear();
  }
}

class WithSet {
  private items: Set<string> = new Set();

  fillAndIter(): void {
    const s = this.items;
    s.add("alpha");
    s.add("beta");
    let count = 0;
    for (const v of s) {
      count++;
    }
    console.log(`set size: ${s.size}, iter: ${count}`);
  }
}

class WithArray {
  private xs: number[] = [];

  fillAndIter(): void {
    const a = this.xs;
    a.push(1);
    a.push(2);
    a.push(3);
    let sum = 0;
    for (const x of a) {
      sum += x;
    }
    console.log(`array length: ${a.length}, sum: ${sum}`);
  }
}

const ex = new Example();
ex.runViaLocal(1025);
ex.runViaDirect(2048);
const ws = new WithSet();
ws.fillAndIter();
const wa = new WithArray();
wa.fillAndIter();

// #305 round 2: inferred-from-initializer shape (no `: Map<...>` annotation).
// Pre-fix, both `for-of this.map` and `for-of local` skipped the loop body.
class InferredMap {
  private map = new Map<number, string>();

  add(k: number, v: string): void {
    this.map.set(k, v);
  }

  iterDirect(): number {
    let count = 0;
    for (const [_k, _v] of this.map) {
      count++;
    }
    return count;
  }

  iterViaLocal(): number {
    const local = this.map;
    let count = 0;
    for (const [_k, _v] of local) {
      count++;
    }
    return count;
  }
}

const im = new InferredMap();
im.add(1, "hello");
im.add(2, "world");
console.log(`inferred direct: ${im.iterDirect()}`);
console.log(`inferred alias:  ${im.iterViaLocal()}`);

class InferredSet {
  private items = new Set<string>();
  iter(): number {
    this.items.add("a");
    this.items.add("b");
    this.items.add("c");
    let n = 0;
    for (const _x of this.items) n++;
    return n;
  }
}
console.log(`inferred set: ${new InferredSet().iter()}`);
