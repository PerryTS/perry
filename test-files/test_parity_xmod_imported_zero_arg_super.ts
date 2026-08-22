import { CrossBase } from "./fixtures/parity_5891/xmod_zero/base.ts";
class CrossSub extends CrossBase {
  constructor() { super(); (this as any).subReady = "sub"; }
}
const value: any = new CrossSub();
console.log("instanceof base", value instanceof CrossBase);
console.log("root", value.rootReady);
console.log("base", value.baseReady);
console.log("sub", value.subReady);
