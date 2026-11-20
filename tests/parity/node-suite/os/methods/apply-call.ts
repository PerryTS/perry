// #1722: indirect invocation of stdlib namespace methods via
// Function.prototype.apply / .call must reach the native impl. Use
// deterministic shape-only assertions so the output matches Node on any
// host (the actual platform/arch strings vary by machine).
import * as os from "node:os";

console.log("platform apply is string:", typeof os.platform.apply(null, []) === "string");
console.log("platform call matches direct:", os.platform.call(null) === os.platform());
console.log("arch apply matches direct:", os.arch.apply(null, []) === os.arch());
console.log("endianness call known:", ["LE", "BE"].includes(os.endianness.call(null)));
console.log("type apply nonempty:", os.type.apply(null, []).length > 0);
console.log("totalmem call is number:", typeof os.totalmem.call(null) === "number");
