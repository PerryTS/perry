// N6, import cycle and imported constants: cyc_b's body runs before cyc_a's
// and calls cyc_a's functions, which read cyc_a's module consts in their
// dead zone: each must throw ReferenceError there, not yield the constant.
// After cyc_a's body has run, the functions and the imported bindings read
// the values.
import { early, lateB, FLAG, WIDTH, readFlag, readWidth, readBoth } from "../../../fixtures/programs/fixtures/module_const_numbers/cyc_a.ts";
import { B_SHIFT } from "../../../fixtures/programs/fixtures/module_const_numbers/cyc_b.ts";

const LOCAL = FLAG | B_SHIFT;
function mix(v: number): number { return (v & FLAG) + (v >> B_SHIFT) + WIDTH + LOCAL; }
class User {
  w = WIDTH * 2;
  has(v: number): boolean { return (v & FLAG) !== 0; }
}
for (const line of early) console.log("early", line);
console.log("late", lateB());
console.log("main", FLAG, WIDTH, B_SHIFT, LOCAL, readFlag(2), readWidth(), readBoth());
console.log("mix", mix(4096), mix(5000), mix(-1));
const u = new User();
console.log("user", u.w, u.has(4096), u.has(4095));
