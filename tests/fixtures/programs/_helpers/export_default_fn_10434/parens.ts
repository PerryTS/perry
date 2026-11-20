// Parenthesized / type-asserted identifier.
function Wrapped(this: any, value?: unknown) {
  this.value = value;
}
Wrapped.prototype.read = function (this: any) {
  return "wrapped:" + typeof this.value;
};
(Wrapped as any).kind = "wrapped-static";
export const holder = { Wrapped };
export default (Wrapped as unknown as typeof Wrapped);
