// An int32 field kept at rest (an I32 lane) of a class defined in a JS
// module: reads and writes in this module must use the lane the writer
// wrote, whatever this module knows of the class (an imported stub).
import { Mask } from "./fixtures/cross_module_rest_lane.js";
import { Mask as Layers } from "./fixtures/cross_module_rest_lane.js";

const m = new Mask();
m.enable(1);
console.log(String(m.mask));

function read(x: Mask): number {
  return x.mask;
}
const a: any = m;
console.log(String(read(m)), String(a.mask), String(a.mask + 1));

const n = new Mask();
n.set(1);
n.mask = n.mask | 1;
console.log(String(n.mask), String(n.test(m)));

const l = new Layers();
l.enable(4);
l.mask = (l.mask ^ 1) | 0;
let acc = 0;
for (let i = 0; i < 4; i++) acc = (acc + l.mask + i) | 0;
console.log(String(l.mask), String(acc), String(l.test(m)));
