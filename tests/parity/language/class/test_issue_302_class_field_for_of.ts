// Regression test for issue #302:
// Map/Set held in a class instance field didn't iterate via for...of.
// Pre-fix the for-of HIR lowering only resolved iterable types via
// `lookup_local_type` (i.e. for `ast::Expr::Ident` iterables), so
// `for (const [k, v] of this.someMap)` produced a raw Map handle that
// the for-loop's `.length` read returned 0 on, silently skipping the
// loop body. v0.5.388 extends the resolver to also look up class field
// types when the iterable is `this.<field>`.

class Example {
  public classMap: Map<number, string> = new Map();
  public classSet: Set<string> = new Set();
  public ages: Map<string, number>;

  constructor() {
    this.ages = new Map();
  }

  populate(): void {
    this.classMap.set(1, "a");
    this.classMap.set(2, "b");
    this.classMap.set(3, "c");
    this.classSet.add("x");
    this.classSet.add("y");
    this.classSet.add("z");
    this.ages.set("alice", 30);
    this.ages.set("bob", 25);
  }

  iterate(): void {
    let mapTotal = 0;
    let mapValues = "";
    for (const [k, v] of this.classMap) {
      mapTotal += k;
      mapValues += v;
    }
    console.log("classMap:", mapTotal, mapValues);

    let setJoined = "";
    for (const v of this.classSet) {
      setJoined += v + ",";
    }
    console.log("classSet:", setJoined);

    // Constructor-initialized field (different code path).
    let agesSum = 0;
    for (const [name, age] of this.ages) {
      agesSum += age;
      console.log("ages entry:", name, age);
    }
    console.log("agesSum:", agesSum);
  }
}

const e = new Example();
e.populate();
e.iterate();
