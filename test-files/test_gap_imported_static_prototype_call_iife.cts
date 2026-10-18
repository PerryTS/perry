const { Base } = require('./fixtures/awb2-static-proto/base.cjs');
const key = ['get', 'PrototypeOf'].join('');
const fnProto = Object[key](() => 0);
const original = fnProto.call;
fnProto.call = function(_receiver) {
  const A = this();
  const B = this();
  return [A, B];
};
let evaluations = 0;
const pair = (function() {
  const tag = ++evaluations;
  class Repeated extends Base { own() { return this.value + 2; } run() { return this.own(); } capture() { return tag; } }
  return Repeated;
}).call(null);
fnProto.call = original;
console.log('identity', pair[0] !== pair[1], pair[0].prototype !== pair[1].prototype);
console.log('captures', new pair[0]().capture(), new pair[1]().capture());
pair[0].prototype.own = () => 71;
console.log('values', new pair[0]().run(), new pair[1]().run());
let thisEvaluations = 0;
const answer = (function(value) { return value + 1; }).call(++thisEvaluations, 7);
console.log('thisArg', thisEvaluations, answer);
