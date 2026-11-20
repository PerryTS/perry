export class Counter {
  static base = 10;
  static reads = 0;
  static add(n: number) { return this.base + n; }
  static plain(n: number) { return n * 3; }
  static get value() { this.reads++; return this.base + this.reads; }
  static get factory() { this.reads++; return function(n: number) { return this.base + n; }; }
}
export class Derived extends Counter {
  static base = 20;
}
export { Counter as Alias };
export default Counter;
