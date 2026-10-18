import { Base } from './fixtures/awb2-static-proto/base.ts';
const Once = (() => {
  class Once extends Base { own() { return this.value + 2; } run() { return this.own(); } }
  return Once;
})();
console.log('iife', new Once().run(), Object.getPrototypeOf(Once.prototype) === Base.prototype);
function make() {
  class Many extends Base { own() { return this.value + 4; } run() { return this.own(); } }
  return Many;
}
const A = make(); const B = make();
console.log('repeated', A !== B, A.prototype !== B.prototype, new A().run(), new B().run());
A.prototype.own = () => 19;
console.log('isolated', new A().run(), new B().run());
