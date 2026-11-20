require('./cycle.cjs');
class Base { constructor() { this.base = 10; } value() { return this.base; } }
exports.Base = Base;
