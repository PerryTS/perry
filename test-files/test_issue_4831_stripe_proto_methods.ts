// Issue #4831: Stripe Node SDK — all resource methods undefined
// (`stripe.products.create` etc. is not a function).
//
// Root cause: the Stripe SDK builds resource methods cross-module via
// `StripeResource.extend({ create: stripeMethod(...) })`. `StripeResource` is a
// plain FUNCTION imported from another module, and `extend`/`method` are
// DYNAMIC own properties (closures) assigned onto it. Perry's HIR lowering
// lifts the call `StripeResource.extend(...)` to a `StaticMethodCall` because
// the receiver is an uppercase imported identifier that "looks like a class".
// At codegen, that StaticMethodCall resolved through none of the known paths
// (it is not a same-module class static, not a namespace import, not a
// V8-fallback specifier), so the fallback returned the literal `0` — the call
// never invoked `protoExtend`, the resource constructor was garbage, and every
// resource method (`create`, `retrieve`, …) was missing/undefined.
//
// Fix: when the StaticMethodCall receiver resolves to a materializable imported
// value (a native imported-function symbol or an imported class-ref), route the
// call through the runtime method dispatcher (`js_native_call_method`), which
// reads the named method off the receiver's dynamic props and invokes it with
// `this` bound to the receiver — the same dispatch the same-module path uses.
// (Related to #4656, the general prototype-chain `[[Get]]` inheritance gap;
// this fix is scoped to the cross-module dynamic-method-on-imported-function
// call shape that Stripe exercises.)
//
// File: crates/perry-codegen/src/expr/static_method.rs
import { Products } from "./fixtures/issue_4831_stripe/products";

// ResourceNamespace-style instantiation: `new resources[name](stripe)` — a
// computed-member `new` whose constructor is the cross-module `extend` result.
const resources: any = { Products: Products };

function ResourceNamespace(this: any, stripe: any, res: any) {
  for (const name in res) {
    if (!Object.prototype.hasOwnProperty.call(res, name)) continue;
    const camel = name[0].toLowerCase() + name.substring(1);
    this[camel] = new res[name](stripe);
  }
}

const stripe: any = new (ResourceNamespace as any)("sk_test_dummy", resources);

console.log("typeof stripe.products?.create:", typeof stripe.products?.create);
console.log("typeof stripe.products?.retrieve:", typeof stripe.products?.retrieve);
console.log("stripe.products.create():", stripe.products.create());
console.log("stripe.products._stripe:", stripe.products._stripe);
console.log("typeof stripe.products._makeRequest:", typeof stripe.products._makeRequest);
