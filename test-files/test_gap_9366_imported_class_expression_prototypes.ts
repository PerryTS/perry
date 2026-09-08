import { Decl, Expr, Anon, localProbe } from "./_helpers/imported_class_expr_9366.ts";
console.log("in-defining-module=" + localProbe());
console.log("importer-Decl=" + typeof Decl.prototype);
console.log("importer-Expr=" + typeof Expr.prototype);
console.log("importer-Anon=" + typeof Anon.prototype);
import * as ns from "./_helpers/imported_class_expr_9366.ts";
import { RenamedExpr, RenamedAnon } from "./_helpers/imported_class_expr_barrel_9366.ts";
import { localExprPrototype, replaceExpr, box, text, accessor, accessorReads,
  untouchedFunction, functionCalls } from "./_helpers/imported_class_expr_9366.ts";

console.log("instance", Object.getPrototypeOf(new Expr()) === Expr.prototype);
console.log("named-instance", Object.getPrototypeOf(new Anon()) === Anon.prototype);
console.log("own", Object.hasOwn(Expr, "prototype"));
console.log("local-identity", Expr.prototype === localExprPrototype());
console.log("namespace", ns.Expr.prototype === Expr.prototype, typeof ns.Expr.prototype);
console.log("barrel", RenamedExpr.prototype === Expr.prototype, typeof RenamedExpr.prototype);
console.log("named-barrel", RenamedAnon.prototype === Anon.prototype, typeof RenamedAnon.prototype);
console.log("computed", Expr["prototype"] === Expr.prototype);
console.log("named-class-name", Anon.name);
console.log("object", box.marker);
console.log("string", text.length);
console.log("accessor", accessor.value, accessorReads);
console.log("function", typeof untouchedFunction.marker, functionCalls);

const first = Expr.prototype;
replaceExpr();
console.log("rebound", typeof Expr.prototype, Expr.prototype !== first,
  Expr.prototype === localExprPrototype());
console.log("barrel-rebound", RenamedExpr.prototype === Expr.prototype);
console.log("namespace-rebound", ns.Expr.prototype === Expr.prototype);
console.log("method", Expr.prototype.m());
