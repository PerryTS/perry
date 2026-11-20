// The export clause precedes the hoisted declaration it names.
export default Later;
export const holder = { Later };
function Later(this: any, value?: unknown) {
  this.value = value;
}
Later.prototype.read = function (this: any) {
  return "later:" + typeof this.value;
};
(Later as any).kind = "later-static";
