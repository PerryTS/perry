// F4: an exported function called from untyped JS with a plain object.
// WPFACTS: site .z off
import { callIt } from "./f4_untyped_helper.js";
export class Zed {
  z: number = 2;
}
export function readZ(x: Zed) {
  return x.z;
}
console.log(readZ(new Zed()), callIt(readZ));
