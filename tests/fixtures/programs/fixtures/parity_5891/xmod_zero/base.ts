import { CrossRoot } from "./root.ts";
export class CrossBase extends CrossRoot {
  constructor() { super(); (this as any).baseReady = "base"; }
}
