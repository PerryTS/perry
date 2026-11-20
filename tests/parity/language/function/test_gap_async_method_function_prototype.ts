// A class method's function object is born with the intrinsic prototype of its
// kind: an async method's is %AsyncFunction.prototype%, a generator method's
// %GeneratorFunction.prototype%, an async generator method's
// %AsyncGeneratorFunction.prototype% (the same as the function and closure forms).
import { types } from "node:util";
const FP = Function.prototype;
const AFP = Object.getPrototypeOf(async function () {});
const GFP = Object.getPrototypeOf(function* () {});
const AGFP = Object.getPrototypeOf(async function* () {});
function kind(f: any): string {
  const p = Object.getPrototypeOf(f);
  return p === FP ? "Function" : p === AFP ? "AsyncFunction" : p === GFP ? "GeneratorFunction" : p === AGFP ? "AsyncGeneratorFunction" : "other";
}
class K {
  m() { return 1; }
  async am() { await 0; return 2; }
  async an() { return 3; }
  *gm() { yield 4; }
  async *agm() { yield 5; }
  static async sam() { await 0; return 6; }
  static async san() { return 7; }
  static *sgm() { yield 8; }
  async #pam() { await 0; return 10; }
  pam() { return this.#pam; }
}
const ka = "cam", kn = "can", kg = "cgm", kag = "cagm", ks = "csam", kp = "cm";
class KC {
  [kp]() { return 21; }
  async [ka]() { await 0; return 22; }
  async [kn]() { return 23; }
  *[kg]() { yield 24; }
  async *[kag]() { yield 25; }
  static async [ks]() { await 0; return 26; }
}
const KCP = KC.prototype as any, KCS = KC as any;
function makeClass() { return class { async am() { await 0; return 11; } }; }
const E = makeClass();
const rows: [string, any][] = [
  ["method", K.prototype.m],
  ["async method (await)", K.prototype.am],
  ["async method (no await)", K.prototype.an],
  ["generator method", K.prototype.gm],
  ["async generator method", K.prototype.agm],
  ["static async method (await)", K.sam],
  ["static async method (no await)", K.san],
  ["static generator method", K.sgm],
  ["private async method", new K().pam()],
  ["class expression async method", E.prototype.am],
  ["computed method", KCP[kp]],
  ["computed async method (await)", KCP[ka]],
  ["computed async method (no await)", KCP[kn]],
  ["computed generator method", KCP[kg]],
  ["computed async generator method", KCP[kag]],
  ["computed static async method", KCS[ks]],
];
for (const [name, f] of rows) {
  let constructs = "constructs";
  try { new f(); } catch (e) { constructs = "not a constructor"; }
  console.log(name + ":", kind(f), types.isAsyncFunction(f), types.isGeneratorFunction(f), constructs);
}
console.log("same as async function:", Object.getPrototypeOf(K.prototype.am) === Object.getPrototypeOf(async function () {}));
(async () => {
  const k = new K();
  console.log("results:", await k.am(), await k.an(), await K.sam(), await K.san(), await k.pam().call(k), await new E().am(),
    [...k.gm()].join(","), [...K.sgm()].join(","));
  for await (const v of k.agm()) console.log("async generator yields", v);
  const kc = new KC() as any;
  console.log("computed results:", kc[kp](), await kc[ka](), await kc[kn](), [...kc[kg]()].join(","), await KCS[ks]());
  for await (const v of kc[kag]()) console.log("computed async generator yields", v);
  console.log("computed returns a promise:", kc[ka]() instanceof Promise, kc[kn]() instanceof Promise);
  console.log("returns a promise:", k.am() instanceof Promise, K.san() instanceof Promise);
})();
