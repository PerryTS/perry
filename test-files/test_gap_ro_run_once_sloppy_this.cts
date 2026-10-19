// Keep this a script: CJS wrapping has a separate baseline strictness discrepancy.
class Base { read() { return 5; } }
class Leaf extends Base {}
function root() { return this; }
console.log(typeof root().Function);
console.log(root() === globalThis);
console.log(new Leaf().read());
