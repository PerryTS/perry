import { EmptyMid } from "./fixtures/parity_5891/xmod_empty/mid.ts";
const direct: any = new EmptyMid("direct");
console.log("direct instanceof mid", direct instanceof EmptyMid);
console.log("direct forwarded", direct.forwarded);
class ExplicitLeaf extends EmptyMid {
  constructor(value: any) { super(value); (this as any).leafReady = "explicit"; }
}
const explicit: any = new ExplicitLeaf("explicit");
console.log("explicit instanceof mid", explicit instanceof EmptyMid);
console.log("explicit forwarded", explicit.forwarded);
console.log("explicit ready", explicit.leafReady);
class DefaultLeaf extends EmptyMid {}
const defaultLeaf: any = new DefaultLeaf("default");
console.log("default instanceof mid", defaultLeaf instanceof EmptyMid);
console.log("default forwarded", defaultLeaf.forwarded);
