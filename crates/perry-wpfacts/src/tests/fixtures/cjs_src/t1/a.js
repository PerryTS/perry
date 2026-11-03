const { Base } = require("./b");
class R { constructor() { this.k = 1; } get() { return this.k; } }
class S extends R { constructor() { super(); this.j = 2; } sum() { return this.get() + this.j; } }
class Main extends Base { go() { const s = new S(); let t = 0; for (let i = 0; i < 10; i++) t += s.sum(); return t; } }
exports.newInstance = () => new Main();
