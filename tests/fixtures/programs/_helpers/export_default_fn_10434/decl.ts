// Control: `export default function F() {}`.
export default function Decl(this: any, value?: unknown) {
  this.value = value;
}
Decl.prototype.read = function (this: any) {
  return "decl:" + typeof this.value;
};
(Decl as any).kind = "decl-static";
export const holder = { Decl };
