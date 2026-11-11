// A generator, async or async-generator function inherits apply/call/bind
// through its kind's intrinsic prototype. Once that prototype is written,
// the by-name call reads the method off the chain instead of answering the
// Function.prototype builtin.
function* gen(v: number) { yield v; }
async function af(v: number) { return v; }
async function* agen(v: number) { yield v; }

function viaApply(fn: any, v: number) { return fn.apply(undefined, [v]); }
function viaCall(fn: any, v: number) { return fn.call(undefined, v); }
function viaBind(fn: any, v: number) { return fn.bind(undefined, v); }

// Unwritten kind prototypes: the builtin answers.
console.log("gen apply", viaApply(gen, 1).next().value);
console.log("gen call", viaCall(gen, 2).next().value);
console.log("gen bind", viaBind(gen, 3)().next().value);

const gp: any = Object.getPrototypeOf(gen);
const ap: any = Object.getPrototypeOf(af);
const agp: any = Object.getPrototypeOf(agen);

gp.apply = function (this: any, _r: any, args: any[]) { return "gen-apply:" + (this === gen) + ":" + args[0]; };
gp.call = function (this: any, _r: any, v: any) { return "gen-call:" + (this === gen) + ":" + v; };
ap.bind = function (this: any, _r: any, v: any) { return "af-bind:" + (this === af) + ":" + v; };
Object.defineProperty(agp, "call", { get() { return () => "agen-call-getter"; }, configurable: true });

console.log(viaApply(gen, 4), gen.apply === gp.apply);
console.log(viaCall(gen, 5));
console.log(viaBind(af, 6));
console.log(viaCall(agen, 7));

// Members the kind prototype did not shadow still inherit the builtin.
console.log("gen bind", viaBind(gen, 8)().next().value);
af.call(undefined, 9).then((v: number) => console.log("af call", v));

// An own property still wins over the written prototype.
function* own(v: number) { yield v; }
(own as any).apply = () => "own-apply";
console.log(viaApply(own, 10));

// Deleting the shadow restores the builtin answer through the chain.
delete gp.apply;
console.log("gen apply after delete", viaApply(gen, 11).next().value);
