// #10434: `function F(){}; export default F;` must export the very function
// object the module calls `F` — prototype methods, statics, identity across
// importers, `instanceof`, and argument padding through a function value.
import Point, { holder as pointHolder, isPoint, makeLocal } from "./_helpers/export_default_fn_10434/ctor.ts";
import kinds, { holder as kindsHolder } from "./_helpers/export_default_fn_10434/plain.ts";
import Later, { holder as laterHolder } from "./_helpers/export_default_fn_10434/hoisted.ts";
import Aliased, { holder as aliasedHolder } from "./_helpers/export_default_fn_10434/hoisted_alias.ts";
import Wrapped, { holder as wrappedHolder } from "./_helpers/export_default_fn_10434/parens.ts";
import Alias, { holder as aliasHolder } from "./_helpers/export_default_fn_10434/alias.ts";
import Decl, { holder as declHolder } from "./_helpers/export_default_fn_10434/decl.ts";
import Klass, { holder as klassHolder } from "./_helpers/export_default_fn_10434/klass.ts";
import arrow, { holder as arrowHolder } from "./_helpers/export_default_fn_10434/arrow.ts";
import Expr, { holder as exprHolder } from "./_helpers/export_default_fn_10434/fexpr.ts";
import withDefaults from "./_helpers/export_default_fn_10434/params.ts";
import {
  PointViaBarrel,
  kindsViaBarrel,
  pointSeenHere,
  kindsSeenHere,
} from "./_helpers/export_default_fn_10434/second_importer.ts";
import * as pointNs from "./_helpers/export_default_fn_10434/ctor.ts";
import { describeBeta } from "./_helpers/export_default_fn_10434/cycle_a.ts";
import { describeAlpha } from "./_helpers/export_default_fn_10434/cycle_b.ts";

// 1. Constructor function with prototype methods and statics.
const P: any = Point;
const p = new P(1);
console.log("ctor identity:", P === pointHolder.Point);
console.log("ctor statics:", P.origin, typeof P.create);
console.log("ctor prototype:", typeof P.prototype.describe, P.prototype.constructor === P);
console.log("ctor instance:", p.describe(), typeof p.y, p instanceof P, isPoint(p));
const local = makeLocal();
console.log("ctor local instance:", local instanceof P, local.describe());
console.log("ctor static factory:", P.create("s").describe(), isPoint(P.create(0)));
console.log("ctor two importers:", P === pointSeenHere(), P === PointViaBarrel, P === pointNs.default);

// 2. Plain function: identity, expando, argument padding through a value.
const k: any = kinds;
console.log("plain identity:", k === kindsHolder.kinds, k === kindsSeenHere(), k === kindsViaBarrel);
console.log("plain label:", k.label);
console.log("plain direct:", kinds(), kinds(1));
console.log("plain via value:", k(), k(1), k(1, "b"));
console.log("plain call/apply:", k.call(null), k.apply(null, [1]), k.call(null, 1, 2, 3));
let padded = "";
for (let i = 0; i < 3; i++) {
  const fn: any = i % 2 === 0 ? kinds : kindsSeenHere();
  padded += fn(i) + ";";
}
console.log("plain loop:", padded);

// Default and rest parameters, called directly and through a value.
const wd: any = withDefaults;
console.log("params direct:", withDefaults(), withDefaults(1, undefined, 7, 8));
console.log("params via value:", wd(), wd(1), wd(1, 2, 3, 4), wd.call(null, "c"));

// 3. Export clause ahead of the hoisted declaration, plain and aliased.
for (const [label, C, held] of [
  ["hoisted default", Later, laterHolder.Later],
  ["hoisted alias", Aliased, aliasedHolder.Aliased],
  ["parenthesized", Wrapped, wrappedHolder.Wrapped],
] as [string, any, any][]) {
  const o = new C();
  console.log(label + ":", C === held, C.kind, typeof C.prototype.read, o.read(), o instanceof C);
}

// 4. Forms that already worked (controls).
for (const [label, C, held] of [
  ["alias control", Alias, aliasHolder.Alias],
  ["decl control", Decl, declHolder.Decl],
  ["fexpr control", Expr, exprHolder.Expr],
] as [string, any, any][]) {
  const o = new C();
  console.log(label + ":", C === held, C.kind, typeof C.prototype.read, o.read(), o instanceof C);
}
const K: any = Klass;
console.log("class control:", K === klassHolder.Klass, K.kind, K.extra, new K().read(), new K() instanceof K);
const a: any = arrow;
console.log("arrow control:", a === arrowHolder.arrow, a.kind, a(), a.call(null, 1));

// 5. Cyclic imports: each side reads the other's default after evaluation.
console.log("cycle:", describeAlpha(), describeBeta());

// 6. A dynamic import's `default` is the same binding as the static import.
async function viaDynamicImport() {
  const ctorNs: any = await import("./_helpers/export_default_fn_10434/ctor.ts");
  const plainNs: any = await import("./_helpers/export_default_fn_10434/plain.ts");
  console.log(
    "dynamic import:",
    ctorNs.default === Point,
    ctorNs.default === ctorNs.holder.Point,
    plainNs.default === kinds,
    plainNs.default.label,
  );
}
viaDynamicImport().then(() => console.log("done"));
