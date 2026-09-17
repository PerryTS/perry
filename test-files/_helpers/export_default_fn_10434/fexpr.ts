// Control: a function expression held in a const.
const Expr: any = function (this: any, value?: unknown) {
  this.value = value;
};
Expr.prototype.read = function (this: any) {
  return "expr:" + typeof this.value;
};
Expr.kind = "expr-static";
export const holder = { Expr };
export default Expr;
