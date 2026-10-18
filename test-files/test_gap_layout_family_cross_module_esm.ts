import DefaultBase, { Base } from './_helpers/awb_families/base.ts';
import { ReBase } from './_helpers/awb_families/reexport.ts';
import { Base as StarBase } from './_helpers/awb_families/star.ts';
class NamedLeaf extends Base { constructor() { super(); this.leaf = 4; } sum() { return this.value() + this.leaf; } }
class DefaultLeaf extends DefaultBase { constructor() { super(); this.other = 5; } }
class ReLeaf extends ReBase { constructor() { super(); this.last = 6; } }
class StarLeaf extends StarBase { constructor() { super(); this.star = 7; } }
function choose() { return Base; }
class DynamicLeaf extends choose() { constructor() { super(); this.dynamic = 8; } }
const n = new NamedLeaf();
console.log(n.sum(), Object.keys(n).join(','));
console.log(new DefaultLeaf().base, new ReLeaf().last, new StarLeaf().star, new DynamicLeaf().dynamic);
