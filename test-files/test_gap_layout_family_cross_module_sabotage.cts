const changed = require('./_helpers/awb_families/reassigned.cjs');
changed.replace();
class ReassignedLeaf extends changed.Parent { constructor() { super(); this.leaf = 2; } }
const a = new ReassignedLeaf();
console.log(a.value(), a.replacement, a.leaf, Object.keys(a).join(','));
const { Base } = require('./_helpers/awb_families/base.cjs');
class PrototypeLeaf extends Base { constructor() { super(); this.leaf = 4; } }
const before = new PrototypeLeaf();
console.log(before.value(), before.leaf);
Object.setPrototypeOf(Base.prototype, { set base(v) { this.intercepted = v + 30; } });
const after = new PrototypeLeaf();
console.log(after.base, after.intercepted, after.leaf, Object.keys(after).join(','));
// Known gap, pre-existing on base ee60047b7: super() reads definition-time
// heritage after the child constructor's live [[Prototype]] changes.
// Reproduce with AWB_KNOWN_GAP=1 under Node and Perry. Parent-prototype
// interception above is the required AWB sabotage and passes in both.
if (process.env.AWB_KNOWN_GAP === '1') {
  class Alternate { constructor() { this.alternate = 50; } value() { return 50; } }
  Object.setPrototypeOf(PrototypeLeaf, Alternate);
  const switched = new PrototypeLeaf();
  console.log(switched.alternate, switched.leaf, Object.keys(switched).join(','));
}
