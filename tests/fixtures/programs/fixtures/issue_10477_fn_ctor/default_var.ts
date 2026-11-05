// `var D = factory(); export default D` — decimal.js's module shape.
function factory() {
  function Dec(this: any, v: number): any {
    if (!(this instanceof Dec)) return new (Dec as any)(v);
    this.v = v;
  }
  Dec.prototype = { constructor: Dec };
  return Dec;
}
var D: any = factory();
export default D;
