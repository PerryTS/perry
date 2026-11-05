// Issue #321 (effect `SortedSet` / `RedBlackTree` iteration): a TypeScript
// constructor parameter property whose declared type is an array
// (`constructor(readonly stack: Node[])`) must propagate that declared type
// to a local alias so element indexing takes the array fast path.
//
// Perry registered param-prop fields in the class `fields` list, but NOT in
// the early `class_field_types` registry that `infer_type_from_expr` consults
// for `this.<field>`. So `const stack = this.stack` inferred the alias as
// `Any`, and `stack[stack.length - 1]` read garbage objects (key/right came
// back undefined/null). effect's `RedBlackTreeIterator` has `readonly stack:
// Array<Node<K,V>>` and `moveNext()` does exactly `const stack = this.stack;
// ...; stack[stack.length - 1]` — so its in-order walk lost the last node and
// `Array.from(sortedSet)` returned a short/[null,null] array.
//
// Node's --experimental-strip-types can't run parameter properties, so this
// is a perry-only expected-output test (compared against the stored
// test-parity/expected/test_gap_param_prop_array_index.txt).
//
// Expected output:
// alias top key: 3
// alias right: 5
// after push len: 3 top: 9
// after pop  len: 2 top: 3
// inorder: [1,2,3]

interface Node {
  key: number;
  left: Node | undefined;
  right: Node | undefined;
}

class Walker {
  count = 0;
  constructor(
    readonly self: unknown,
    readonly stack: Node[],
    readonly dir: number,
  ) {}

  // local alias of the param-prop array field, then element index — the
  // exact RedBlackTree moveNext shape.
  topKey(): number {
    const stack = this.stack;
    const n = stack[stack.length - 1];
    return n.key;
  }

  topRight(): number {
    const stack = this.stack;
    const n = stack[stack.length - 1];
    return n.right != null ? n.right.key : -1;
  }

  push(n: Node): void {
    this.count++;
    const stack = this.stack;
    stack.push(n);
  }

  pop(): void {
    const stack = this.stack;
    stack.pop();
  }

  len(): number {
    return this.stack.length;
  }
}

const n1: Node = { key: 1, left: undefined, right: undefined };
const n5: Node = { key: 5, left: undefined, right: undefined };
const n3: Node = { key: 3, left: undefined, right: n5 };

const w = new Walker(null, [n1, n3], 0);
console.log("alias top key:", w.topKey());
console.log("alias right:", w.topRight());

const n9: Node = { key: 9, left: undefined, right: undefined };
w.push(n9);
console.log("after push len:", w.len(), "top:", w.topKey());
w.pop();
console.log("after pop  len:", w.len(), "top:", w.topKey());

// Full in-order traversal driven by a param-prop iterator (the RBTree shape).
class InOrderIter {
  count = 0;
  constructor(
    readonly self: unknown,
    readonly stack: Node[],
    readonly dir: number,
  ) {}

  next(): { done: boolean; value: number } {
    let value = -1;
    let done = true;
    if (this.stack.length > 0) {
      value = this.stack[this.stack.length - 1].key;
      done = false;
    }
    this.count++;
    this.moveNext();
    return { done, value };
  }

  moveNext(): void {
    const stack = this.stack;
    if (stack.length === 0) return;
    let n: Node | undefined = stack[stack.length - 1];
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

const a1: Node = { key: 1, left: undefined, right: undefined };
const a3: Node = { key: 3, left: undefined, right: undefined };
const a2: Node = { key: 2, left: a1, right: a3 };
const initial: Node[] = [];
let cur: Node | undefined = a2;
while (cur != null) {
  initial.push(cur);
  cur = cur.left;
}
const it = new InOrderIter(null, initial, 0);
const out: number[] = [];
for (let i = 0; i < 10; i++) {
  const r = it.next();
  if (r.done) break;
  out.push(r.value);
}
console.log("inorder:", JSON.stringify(out));
