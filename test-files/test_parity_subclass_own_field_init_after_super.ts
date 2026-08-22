abstract class Base {
  _def: any;
  constructor(def: any) { this._def = def; }
  abstract _p(): any;
}
class Leaf extends Base {
  _cached: any = null;
  _count = 7;
  _list: number[] = [1, 2];
  _p() { return 1; }
  static create = (): Leaf => new Leaf({ k: 1 });
}
const o: any = Leaf.create();
if (o._cached !== null || o._cached === 0) throw new Error("_cached: " + o._cached);
if (o._count !== 7) throw new Error("_count: " + o._count);
if (JSON.stringify(o._list) !== "[1,2]") throw new Error("_list: " + JSON.stringify(o._list));
if (JSON.stringify(o._def) !== '{"k":1}') throw new Error("_def: " + JSON.stringify(o._def));
console.log("OK");
