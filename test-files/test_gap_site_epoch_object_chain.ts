// Plain prototype chains built with Object.create: a deep inherited read,
// a deep absent read and a computed-key read stay correct while the chain's
// intermediate objects change after the sites were warmed.
const root: any = { k: "root", n: 1 };
const mid1: any = Object.create(root);
const mid2: any = Object.create(mid1);
const mid3: any = Object.create(mid2);
const leaf: any = Object.create(mid3);
leaf.own = 1;

function named(o: any): string {
  return String(o.k) + "/" + String(o.missing);
}
function keyed(o: any, key: string): string {
  return String(o[key]);
}

for (let i = 0; i < 300; i++) {
  named(leaf);
  keyed(leaf, "k");
  keyed(leaf, "n");
}
console.log("warm", named(leaf), keyed(leaf, "k"), keyed(leaf, "n"));

mid2.k = "mid2";
console.log("mid key added", named(leaf), keyed(leaf, "k"));
mid1.missing = "found";
console.log("absent now present", named(leaf));
Object.defineProperty(mid3, "n", { get: () => 42, configurable: true });
console.log("accessor on hop", keyed(leaf, "n"));
delete mid3.n;
delete mid2.k;
console.log("deleted", named(leaf), keyed(leaf, "k"), keyed(leaf, "n"));
Object.setPrototypeOf(mid1, { k: "other", n: 7 });
console.log("relinked", named(leaf), keyed(leaf, "k"), keyed(leaf, "n"));
root.k = "changed";
console.log("old root changed", named(leaf));
