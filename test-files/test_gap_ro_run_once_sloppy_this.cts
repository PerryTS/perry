const { Base } = require("./fixtures/ro-run-once/base.cjs");
class Leaf extends Base {}
function root() { return this; }
console.log(typeof root().Function);
console.log(root() === globalThis);
console.log(new Leaf().read());
