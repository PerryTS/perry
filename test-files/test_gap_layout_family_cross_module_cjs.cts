const { Base: Parent } = require('./_helpers/awb_families/base.cjs');
const m = require('./_helpers/awb_families/module_base.cjs');
const { WholeBase } = require('./_helpers/awb_families/whole.cjs');
class DestructuredLeaf extends Parent { constructor() { super(); this.leaf = 2; } sum() { return this.value() + this.leaf; } }
class NamespaceLeaf extends m.ModuleBase { constructor() { super(); this.leaf = 3; } }
class WholeLeaf extends WholeBase { constructor() { super(); this.leaf = 4; } }
const a = new DestructuredLeaf();
const b = new NamespaceLeaf();
const c = new WholeLeaf();
console.log(a.sum(), Object.keys(a).join(','));
console.log(b.moduleBase, b.leaf, c.whole, c.leaf);
const reexport = require('./_helpers/awb_families/reexport.cjs');
class ReexportLeaf extends reexport.Base { constructor() { super(); this.reexported = 6; } }
console.log(new ReexportLeaf().value(), new ReexportLeaf().reexported);
