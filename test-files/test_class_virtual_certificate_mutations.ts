class CVBase { scale = 3; step(x: number): number { return this.scale * x + 7; } }
class CVDerived extends CVBase { step(x: number): number { return this.scale * x + 11; } }
function invoke(c: CVBase, x: number): number { const cs: CVBase[] = [c]; return cs[0].step(x); }
const receivers: CVBase[] = [new CVBase(), new CVDerived()];
for (let i = 0; i < 20; i++) invoke(receivers[i & 1], i);
const c: any = receivers[0];
console.log('initial', invoke(c, 2));
c.step = function(x: number) { return x + 50; };
console.log('own', invoke(c, 2));
delete c.step;
console.log('deleted', invoke(c, 2));
Object.defineProperty(c, 'step', { configurable: true, value: function(x: number) { return x + 70; } });
console.log('define', invoke(c, 2));
delete c.step;
const p: any = CVBase.prototype;
p.step = function(x: number) { return x + 90; };
console.log('proto', invoke(c, 2));
Object.setPrototypeOf(c, { step(x: number) { return x + 110; } });
console.log('edge', invoke(c, 2));
const subclassPrototype: any = CVDerived.prototype;
subclassPrototype.scale = 4;
console.log('subproto', invoke(subclassPrototype, 2));
const created: any = Object.create(CVBase.prototype);
created.step = function(x: number) { return x + 130; };
console.log('created-own', invoke(created, 2));
let gets = 0;
Object.defineProperty(c, 'step', { configurable: true, get() { gets++; return function(x: number) { return x + 150; }; } });
console.log('getter', invoke(c, 2), gets);
class CVSuper extends CVDerived { step(x: number): number { return super.step(x) + 1; } }
console.log('super', invoke(new CVSuper(), 2));
