const { Base } = require('./base.cjs');
class Reload extends Base { own() { return this.value + 2; } run() { return this.own(); } }
exports.Class = Reload;
exports.make = () => new Reload();
