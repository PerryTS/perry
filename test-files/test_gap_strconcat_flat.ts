// Boundary pairs, lone surrogates, erased types, and coercing character indices.
function add(a: any, b: any): any { return a + b; }
function length(v: any): any { return v.length; }
function code(v: any, i: any): any { return v.charCodeAt(i); }
function units(s: any): string {
    let result = '';
    for (let i = 0; i < s.length; i++) result += s.charCodeAt(i) + ',';
    return result;
}
const parts = ['', 'abc', 'abcdef', 'é中🙂', '\ud83d', '\ude00', '\ud83dx', 'x\ude00'];
for (const a of parts) {
    for (const b of parts) {
        const s = add(a, b);
        console.log(s.length, units(s), s.isWellFormed());
    }
}
for (const value of [0, 12.5, NaN, Infinity, true, false, 123n, Symbol('x')]) {
    console.log(typeof length(value));
}
for (const value of [null, undefined]) {
    try { console.log(length(value)); } catch (e) { console.log(e instanceof TypeError); }
    try { console.log(code(value, 0)); } catch (e) { console.log(e instanceof TypeError); }
}
try { console.log(code(42, 0)); } catch (e) { console.log(e instanceof TypeError); }
for (const s of ['abc', 'abcdefghij', 'é🙂中', '\ud83d', '\ude00']) {
    for (const i of [-1, -0.5, 0, 1, 1.9, 5, 100, NaN, Infinity, undefined, '1', true]) {
        console.log(code(s, i));
    }
}
let calls = '';
const index = { valueOf() { calls += 'i'; return 1; } };
console.log(code('abcdef', index), calls);
console.log(code({charCodeAt(i: any) { return i + 40; }}, 2));
console.log(add({valueOf() { calls += 'a'; return 'left'; }},
    {valueOf() { calls += 'b'; return 'right'; }}), calls);
console.log(add(20, 22), add('x', 42), add(42, 'x'));
Object.defineProperty(Number.prototype, 'length', {configurable: true, get() {
    'use strict'; return typeof this === 'number' ? 73 : 74;
}});
console.log(length(42));
delete (Number.prototype as any).length;
(Object.prototype as any).length = 91;
console.log(length(42), length(true), length(123n));
delete (Object.prototype as any).length;
console.log(typeof length(42));
let long = 'a'.repeat(1_000_000) + '\ud83d';
long = add(long, '\ude00');
console.log(long.length, code(long, 999999), code(long, 1000000), code(long, 1000001));
// Source-level rope building is represented as flat strings in Perry.
let chain: any = '';
for (let i = 0; i < 1000; i++) chain = add(chain, i % 2 ? '\ude00' : '\ud83d');
console.log(chain.length, chain.isWellFormed(), code(chain, 998), code(chain, 999));

// Joined producers preserve character reads and repair all surrogate seams.
for (const a of parts) {
    for (const b of parts) {
        for (const separator of ['', 'x', '\ud83d', '\ude00']) {
            const s: any = [a, '', b].join(separator);
            console.log(s.length, units(s), s.isWellFormed());
        }
    }
}
