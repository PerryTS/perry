const { Base } = require("./base.cjs");
class Leaf extends Base { read() { return super.read() + 1; } }
module.exports = { Leaf, make: () => new Leaf() };
