// An int32 field kept at rest (an I32 lane) of a class born at its static
// family layout whose key colouring moves another key into the field's
// position: the field's store must not land in that other key's slot.
import { Caster, Pair, Triple } from "./fixtures/family_rest_lane_slot.js";

function cameraOf(o: any): any {
  return o.camera;
}
function rayOf(o: any): any {
  return o.ray;
}
const others: any[] = [new Pair(), new Triple(), JSON.parse('{"camera":7,"ray":8}')];
let sum = 0;
for (const o of others) sum += cameraOf(o) + rayOf(o);

const c = new Caster();
c.aim({ v: 3 });
let acc = 0;
for (let i = 0; i < 4; i++) acc = (acc + (c.near | 0) + i) | 0;
console.log(String(c.ray.origin.x), String(c.near), String(c.far), String(sum), String(acc));
console.log(typeof c.ray, c.camera !== null, typeof c.ray.direction);

// Writer in the fixture module, readers here and there.
c.setNear(5);
const n: number = c.near;
console.log(String(n * 2), String(c.nearTimes(3)), String(c.ray.origin.x), typeof c.ray.direction);
