class Base {}
class Cache extends Base { constructor() { super(); } }
const factory = () => new Cache();
const value = factory();
if (!(value instanceof Cache)) throw new Error("expected Cache instance");
console.log("derived constructor arrow new ok");
