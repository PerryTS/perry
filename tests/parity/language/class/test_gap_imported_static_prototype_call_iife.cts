const { Base } = require('../../../fixtures/programs/fixtures/awb2-static-proto/base.cjs');
const key = ['get', 'PrototypeOf'].join('');
const fnProto = Object[key](() => 0);
const original = fnProto.call;
try { fnProto.call = function(_receiver) {
  const A = this();
  const B = this();
  return [A, B];
}; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
let evaluations = 0;
const pair = (function() {
  const tag = ++evaluations;
  class Repeated extends Base { own() { return this.value + 2; } run() { return this.own(); } capture() { return tag; } }
  return Repeated;
}).call(null);
try { fnProto.call = original; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
// DEC 250 keeps the original call, so `pair` is the class itself and
// `pair[0]` is undefined: the first read below throws.
try {
  console.log('identity', pair[0] !== pair[1], pair[0].prototype !== pair[1].prototype);
  console.log('captures', new pair[0]().capture(), new pair[1]().capture());
  pair[0].prototype.own = () => 71;
  console.log('values', new pair[0]().run(), new pair[1]().run());
} catch (e) { console.log((e as Error).constructor.name + ": " + (e as Error).message); }
let thisEvaluations = 0;
const answer = (function(value) { return value + 1; }).call(++thisEvaluations, 7);
console.log('thisArg', thisEvaluations, answer);
