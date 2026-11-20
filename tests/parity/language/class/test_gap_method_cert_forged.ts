// Class dispatch with two receiver classes at one site: every build must call
// each receiver's own body (object model step 3: a branch over the class id
// against the prototype constants the write census proves).
class Shape { area() { return 0; } }
class Sq extends Shape { s = 3; area() { return this.s * this.s; } }
class Ci extends Shape { r = 2; area() { return 3 * this.r * this.r; } }
class Tri extends Shape { b = 4; h = 5; }
function area(o: any) { return o.area(); }
const xs: any[] = [new Sq(), new Ci(), new Tri(), new Sq(), new Ci()];
let sum = 0;
for (let r = 0; r < 50; r++) for (const o of xs) sum += area(o);
console.log("sum", sum);
console.log(xs.map(area).join(","));
// Same class, a different own layout: another ShapeId, another word.
const wide: any = new Sq();
wide.extra = 1;
console.log("wide", area(wide), area(xs[0]));
// An own override is never certified.
wide.area = () => 42;
console.log("own", area(wide), area(xs[0]));
