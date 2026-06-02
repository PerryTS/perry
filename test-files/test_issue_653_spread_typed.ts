// Issue #653: AOT spread of non-empty array passes denormal placeholders to callee.
function f(name: string, ...args: string[]): string[] {
    return [name, ...args];
}

const empty: string[] = [];
const one: string[] = ['x'];
const two: string[] = ['x', 'y'];

console.log('direct:        ', f('a'));
console.log('spread empty:  ', f('a', ...empty));
console.log('spread 1:      ', f('a', ...one));
console.log('spread 2:      ', f('a', ...two));
