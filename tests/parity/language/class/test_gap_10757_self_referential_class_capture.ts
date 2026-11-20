// #10757: HIR lowering never finishes for a class whose own methods each
// construct a fresh instance of itself while the class also captures an
// outer local (`class Cell { get value() { return this.v + base; }
// plus1() { return new Cell(this.v + 1); } ... }` inside a factory
// function). Every `new Cell(...)` inside Cell's own methods forwards
// Cell's captured outer local, which `shared_mutable_capture.rs`'s
// `for_each_nested_capture` misreads as Cell being NESTED inside Cell,
// recursing `class_mutates_capture` back into Cell itself. Each additional
// self-constructing method adds a branch to that self-recursion, capped
// only by MAX_NESTED_CLASS_DEPTH (8) — so lowering this single, ordinary,
// ~20-line factory function is exponential in its method count and never
// finished within any reasonable time on unfixed `main` (bisected from a
// real-world hang compiling `@noble/curves`' weierstrass.js from `ethers`
// 6.17.0, where the shape is the same: a `Point` class whose arithmetic
// methods each return `new Point(...)`).
function makeCounter() {
  let base = 10;
  class Cell {
    v: number;
    constructor(v: number) { this.v = v; }
    get value(): number { return this.v + base; }
    plus1(): Cell { return new Cell(this.v + 1); }
    plus2(): Cell { return new Cell(this.v + 2); }
    plus3(): Cell { return new Cell(this.v + 3); }
    plus4(): Cell { return new Cell(this.v + 4); }
    plus5(): Cell { return new Cell(this.v + 5); }
    plus6(): Cell { return new Cell(this.v + 6); }
    plus7(): Cell { return new Cell(this.v + 7); }
    plus8(): Cell { return new Cell(this.v + 8); }
  }
  return new Cell(0);
}

const c = makeCounter();
console.log(c.plus1().plus2().plus3().value);
