// The third way to forward a name: an Import plus `Export::Named`. The other
// two (`export { NS } from "./ns.ts"` and `export *`) were already followed.
import { NS } from "./ns.ts";
export { NS };
