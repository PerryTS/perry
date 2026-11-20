"use strict";
function cold(n: number, value: number) {
    if (n < 0) return arguments;
    return value;
}
console.log('cold', cold(1, 7));
const escaped = cold(-1, 9) as IArguments;
console.log('escaped', escaped.length, escaped[0], escaped[1], Array.isArray(escaped));
function strict(a: number) {
    a = 8;
    const first = arguments;
    first[0] = 6;
    first.extra = 19;
    console.log('strict', a, first[0], first === arguments, arguments.extra);
    console.log('keys', Object.keys(first).join(','));
    console.log('length', Object.getOwnPropertyDescriptor(first, 'length')!.enumerable);
    try { first.callee; } catch (e) { console.log('callee', e instanceof TypeError); }
    delete first[0];
    console.log('delete', 0 in first, first.length);
}
strict(2);
function capture(a: number) {
    const f = () => arguments;
    a = 99;
    return f;
}
const f = capture(23);
console.log('capture', f()[0], f() === f());
function captureLength(a: number) { return () => arguments.length; }
const lengthAfterReturn = captureLength(1, 2, 3);
console.log('capture-length', lengthAfterReturn());
function recursive(n: number): number {
    const before = arguments;
    if (n === 0) return before[0];
    return recursive(n - 1) + before[0] + (before === arguments ? 1 : 1000);
}
console.log('recursive', recursive(5));
// Strict mode forbids assigning the binding; the sloppy witness lives in the
// companion file instead. Defaults observe the same pre-body snapshot.
function defaults(a: number = arguments[1], b: number = 4) { return arguments; }
const d = defaults(undefined, 11);
console.log('defaults', d.length, d[0], d[1]);
function rest(a: number, ...values: number[]) {
    const args = arguments;
    values[0] = 50;
    return args;
}
console.log('rest', rest(1, 2, 3)[1]);
class C {
    method(a: number) { return () => arguments; }
    static method(a: number) { if (a < 0) return arguments; return a; }
}
const c = new C();
const methodArgs = c.method(31);
console.log('method', methodArgs()[0], methodArgs() === methodArgs());
console.log('static', C.method(3), (C.method(-3) as IArguments)[0]);
class Keep {
    args: IArguments;
    constructor(a: number) { this.args = arguments; }
}
const kept: Keep[] = [];
for (let i = 0; i < 4; i++) kept.push(new Keep(i));
console.log('constructors', kept.map(k => k.args[0]).join(','));
function* generator(a: number) { yield arguments[0]; yield arguments.length; }
const iterator = generator(17, 21);
console.log('generator', iterator.next().value, iterator.next().value);
async function afterAwait(a: number) { await Promise.resolve(); return arguments[0]; }
afterAwait(29).then(value => console.log('async', value));
