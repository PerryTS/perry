// #321 (effect `SortedSet` / `RedBlackTree` iteration): indexing into a
// class instance array field named `stack` / `name` / `message` must read a
// real array element, NOT a string character.
//
// Perry has a name-only heuristic that classifies `<x>.message` / `<x>.stack`
// / `<x>.name` as a string (for caught-Error `.stack.includes(...)` chains).
// That heuristic wrongly hijacked a USER class whose own field happened to be
// named `stack` with a non-string declared type: `this.stack[i]` lowered as a
// string `char_at` and returned garbage. effect's `RedBlackTreeIterator` has
// `readonly stack: Array<Node<K,V>>`, so `this.stack[this.stack.length - 1]`
// read bogus nodes — SortedSet iteration came back as `[null, null]`.
//
// The fix: when the receiver is a known class/interface that declares a field
// with that name, the field's declared type wins over the Error heuristic.
//
// Compared byte-for-byte against `node --experimental-strip-types`.

interface TreeNode {
  key: number;
  left: TreeNode | undefined;
  right: TreeNode | undefined;
}

// A class with array/object fields named exactly `stack`, `name`, `message`.
class Walker {
  stack: TreeNode[];
  name: number[];
  message: { v: number }[];
  constructor(stack: TreeNode[], name: number[], message: { v: number }[]) {
    this.stack = stack;
    this.name = name;
    this.message = message;
  }
  // (1) direct `this.stack[i]` index — must read a node, not a char.
  topKey(): number {
    return this.stack[this.stack.length - 1].key;
  }
  // (2) `this.name[i]` numeric-array index.
  firstName(): number {
    return this.name[0];
  }
  // (3) `this.message[i].v` index-then-member.
  firstMessageV(): number {
    return this.message[0].v;
  }
  // (4) local alias of the array field, then element index — the moveNext
  //     shape (`const stack = this.stack; stack[stack.length - 1]`).
  topKeyViaAlias(): number {
    const stack = this.stack;
    const n = stack[stack.length - 1];
    return n.key;
  }
}

const n1: TreeNode = { key: 1, left: undefined, right: undefined };
const n3: TreeNode = { key: 3, left: undefined, right: undefined };
const n2: TreeNode = { key: 2, left: n1, right: n3 };

const w = new Walker([n2, n1], [10, 20], [{ v: 7 }, { v: 8 }]);
console.log("topKey:", w.topKey());
console.log("firstName:", w.firstName());
console.log("firstMessageV:", w.firstMessageV());
console.log("topKeyViaAlias:", w.topKeyViaAlias());

// (5) The genuine Error `.message` / `.stack` string path must STILL work —
//     the heuristic only yields to *declared* class fields.
try {
  throw new Error("boom");
} catch (e) {
  const err = e as Error;
  console.log("err.message len:", err.message.length);
  console.log("err.message upper:", err.message.toUpperCase());
}

// (6) An in-order stack walk that pushes/pops the array field via a local
//     alias and reads `node.right` identity — the RedBlackTree moveNext.
class InOrder {
  stack: TreeNode[];
  out: number[];
  constructor(root: TreeNode) {
    this.stack = [];
    this.out = [];
    let n: TreeNode | undefined = root;
    while (n != null) {
      this.stack.push(n);
      n = n.left;
    }
  }
  run(): number[] {
    while (this.stack.length > 0) {
      const top = this.stack[this.stack.length - 1];
      this.out.push(top.key);
      this.moveNext();
    }
    return this.out;
  }
  moveNext(): void {
    const stack = this.stack;
    if (stack.length === 0) return;
    let n: TreeNode | undefined = stack[stack.length - 1];
    if (n.right != null) {
      n = n.right;
      while (n != null) {
        stack.push(n);
        n = n.left;
      }
    } else {
      stack.pop();
      while (stack.length > 0 && stack[stack.length - 1].right === n) {
        n = stack[stack.length - 1];
        stack.pop();
      }
    }
  }
}

console.log("inorder:", JSON.stringify(new InOrder(n2).run()));
