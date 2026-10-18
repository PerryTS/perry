const { Base } = require('./fixtures/awb2-static-proto/base.cjs');
const key = ['get', 'PrototypeOf'].join('');
const fnProto = Object[key](() => 0);
const original = fnProto.call;
fnProto.call = function(_receiver) {
  const A = this();
  const B = this();
  return [A, B];
};
const pair = (function() {
  class Repeated extends Base { own() { return this.value + 2; } run() { return this.own(); } }
  return Repeated;
}).call(null);
fnProto.call = original;
console.log('identity', pair[0] !== pair[1], pair[0].prototype !== pair[1].prototype);
pair[0].prototype.own = () => 71;
console.log('values', new pair[0]().run(), new pair[1]().run());
