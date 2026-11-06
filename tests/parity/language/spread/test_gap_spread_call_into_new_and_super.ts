// Spread into `new` and `super(...args)`, from an array, a class generator and a string.
class Base { constructor(a, b, c, d) { this.s = [a, b, c, d].map(String).join(","); } }
class Der extends Base { constructor(...args) { super(...args, "t"); } }
class G { constructor() { this.x = 1; this.y = 2; } *[Symbol.iterator]() { yield this.x; yield this.y; } }
console.log(new Base(...[1, 2], 3).s, new Base(...new G(), 3).s, new Base(..."ab").s);
console.log(new Der(...[1, 2]).s, new Der(...new G()).s, new Der(..."xyz").s);
const C = Base;
console.log(new C(...new G(), ...[7]).s);
