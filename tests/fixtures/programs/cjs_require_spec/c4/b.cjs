'use strict';
class Pt { constructor(x, y) { this.x = x; this.y = y; } sum() { return this.x + this.y; } }
function mk(a) { const o = { x: a, y: 2 }; return o.x * o.y; }
module.exports = { Pt, mk };
