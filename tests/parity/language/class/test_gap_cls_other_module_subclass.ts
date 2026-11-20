// CLS D1: a subclass declared in another module (a unit the importing unit
// cannot see) overrides the method; a proof over this module's classes alone
// must not close the site.
import { Base, Other, makeSub } from "./gap_cls_other_module_subclass_helper.ts";
class Mine extends Base {
  area(): number { return 3; }
}
class Mine2 extends Base {
  area(): number { return 4; }
}
const Sub = makeSub(Mine2);
const items: Base[] = [new Mine(), new Mine2(), new Other(), new Sub()];
function total(): number {
  let t = 0;
  for (let i = 0; i < items.length; i++) {
    const s: Base = items[i];
    t += s.describe();
  }
  return t;
}
console.log(total());
console.log(items.map((b) => b.describe()).join(","));
