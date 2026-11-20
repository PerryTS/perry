function a(n: number) { return { value: n + 1 }; }
function b(n: number) { return { value: n + 2 }; }
function c(n: number) { return { value: n + 3 }; }
function d(n: number) { return { value: n + 4 }; }
function e(n: number) { return { value: n + 5 }; }
function f(n: number) { return { value: n + 6 }; }
function g(n: number) { return { value: n + 7 }; }
function h(n: number) { return { value: n + 8 }; }
const functions = [a, b, c, d, e, f, g, h];
let sum = 0;
for (let i = 0; i < functions.length; i++) {
    sum += functions[i](i).value;
}
if (sum !== 64) throw new Error('split unit result changed');
console.log('PASS: split units', sum);
