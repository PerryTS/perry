// Same, through an alias clause ahead of the declaration.
export { Aliased as default };
export const holder = { Aliased };
function Aliased(this: any, value?: unknown) {
  this.value = value;
}
Aliased.prototype.read = function (this: any) {
  return "aliased:" + typeof this.value;
};
(Aliased as any).kind = "aliased-static";
