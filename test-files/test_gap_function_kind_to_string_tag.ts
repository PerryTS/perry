async function af() {}
async function awaited() { await 1; }
function* gf() { yield 1; }
async function* agf() { yield 1; }
const asyncArrow = async () => { await 1; };
const values: any[] = [af, gf, agf, asyncArrow, awaited, function ordinary() {}];
for (const value of values) console.log(Object.prototype.toString.call(value));
const ap: any = Object.getPrototypeOf(af);
Object.defineProperty(ap, Symbol.toStringTag, { value: 'Changed', configurable: true });
console.log('changed inherited', Object.prototype.toString.call(af));
Object.defineProperty(ap, Symbol.toStringTag, { get() { return this === af ? 'Receiver' : 'Wrong'; }, configurable: true });
console.log('inherited getter', Object.prototype.toString.call(af));
Object.defineProperty(af, Symbol.toStringTag, { value: 'Own', configurable: true });
console.log('own', Object.prototype.toString.call(af));
delete (af as any)[Symbol.toStringTag];
delete ap[Symbol.toStringTag];
console.log('deleted inherited', Object.prototype.toString.call(af));
const custom = Symbol('custom');
ap[custom] = 7;
console.log('inherited symbol', (af as any)[custom]);
const fp: any = Object.getPrototypeOf(values[5]);
Object.defineProperty(fp, Symbol.toStringTag, { value: 'Parent', configurable: true });
console.log('ordinary parent', Object.prototype.toString.call(values[5]));
console.log('async parent', Object.prototype.toString.call(af));
delete fp[Symbol.toStringTag];
