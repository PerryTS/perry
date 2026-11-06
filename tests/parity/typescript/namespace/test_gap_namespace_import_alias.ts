// `import G = Ns.Member` aliases a namespace member; tsc emits
// `var G = Ns.Member`. The declaration was dropped, so every use was a
// ReferenceError ("G is not defined").
namespace Geo {
  export const PI2 = Math.PI * 2;
  export namespace Inner {
    export let count = 0;
    export function bump() {
      return ++count;
    }
  }
  export class Pt {
    constructor(public x: number) {}
  }
}
import G = Geo.Inner;
import P2 = Geo.PI2;
import Pt = Geo.Pt;
import Whole = Geo;
export import Exported = Geo.Inner;
console.log(G.bump(), G.bump(), G.count, Geo.Inner.count, P2.toFixed(3), new Pt(4).x, Whole.Inner === G, Exported.bump());
function later() {
  return Late.v;
}
namespace Src {
  export const v = "late";
}
import Late = Src;
console.log(later());
