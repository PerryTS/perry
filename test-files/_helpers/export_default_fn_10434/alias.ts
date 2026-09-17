// Control: `export { F as default }` after the declaration.
function Alias(this: any, value?: unknown) {
  this.value = value;
}
Alias.prototype.read = function (this: any) {
  return "alias:" + typeof this.value;
};
(Alias as any).kind = "alias-static";
export const holder = { Alias };
export { Alias as default };
