// A family class whose constructor is a complete run of parameter stores is
// born with its values written in the nursery birth: pointer fields across
// collections, key order and presence, a long-lived tree beside young ones.
class TreeNode {
  left: TreeNode | null;
  right: TreeNode | null;
  constructor(left: TreeNode | null, right: TreeNode | null) {
    this.left = left;
    this.right = right;
  }
}
class Pair {
  a: any;
  b: any;
  constructor(a: any, b: any) {
    this.a = a;
    this.b = b;
  }
}
function make(depth: number): TreeNode {
  if (depth === 0) return new TreeNode(null, null);
  return new TreeNode(make(depth - 1), make(depth - 1));
}
function check(node: TreeNode): number {
  const l = node.left;
  const r = node.right;
  if (l === null || r === null) return 1;
  return 1 + check(l) + check(r);
}
const longLived = make(14);
let total = 0;
for (let i = 0; i < 40; i++) total += check(make(10));
const pairs: Pair[] = [];
for (let i = 0; i < 20000; i++) pairs.push(new Pair({ i }, "s" + i));
let sum = 0;
for (const p of pairs) sum += p.a.i + p.b.length;
const leaf = new TreeNode(null, null);
console.log(total, check(longLived), sum);
console.log(Object.keys(leaf).join(","), Object.keys(pairs[7]).join(","), JSON.stringify(pairs[3]));
console.log("left" in leaf, leaf.hasOwnProperty("right"), Object.getOwnPropertyNames(longLived).length);
