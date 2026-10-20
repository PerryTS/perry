export const Foo = (function () {
  return class Base { v: number; constructor(v: number) { this.v = v; } };
})();
export const Bar = class Base2 { v: number; constructor(v: number) { this.v = v; } };
export const Decl = (function () {
  class Base { v: number; constructor(v: number) { this.v = v; } }
  return Base;
})();
export function expressionFactory() {
  return class Base { v: number; constructor(v: number) { this.v = v; } };
}
export function declarationFactory() {
  class Base { v: number; constructor(v: number) { this.v = v; } }
  return Base;
}
export const WithHeritage = (function () {
  return class Base extends globalThis.Error {};
})();
