// Issue #4841: Stripe SDK — `stripe.<resource>.<method>` dispatched to the
// WRONG resource (`stripe.products.create` ran webhook_endpoints' method),
// surfacing on Linux as `TypeError: replace is not a function` deep in the
// request layer.
//
// Root cause (a #4831 follow-up): Stripe builds every resource with
//   const Constructor = function (...a) { Super.apply(this, a); };
//   Constructor.prototype = Object.create(Super.prototype);
//   Object.assign(Constructor.prototype, methods);   // {create, retrieve, …}
//   return Constructor;
// `Constructor` is a non-arrow function expression that captures only the
// constant `Super`. Perry's closure allocator shared ONE cached function
// object (and therefore ONE `.prototype`) for every evaluation with the same
// (func_ptr, capture-bits) key, so all resources aliased a single prototype.
// Each `Object.assign(Constructor.prototype, methods)` clobbered the prior
// resource's methods, leaving every `stripe.<resource>.<method>` pointing at
// the last-registered resource.
//
// Fix: a non-arrow `function` expression that could be used as a constructor
// (no captures, or capturing an unboxed value) gets a FRESH function object —
// and a fresh `.prototype` — on every evaluation, per JS identity semantics.
// (crates/perry-codegen/src/expr/closure.rs)

function makeMethod(spec: any) {
  return function (this: any) {
    return spec.path;
  };
}

function Base(this: any) {}
(Base as any).prototype = { basePath: "/v1" };

function protoExtend(this: any, sub: any) {
  const Super = this;
  const Constructor: any = function (this: any) {
    Super.apply(this, arguments);
  };
  Object.assign(Constructor, Super);
  Constructor.prototype = Object.create(Super.prototype);
  Object.assign(Constructor.prototype, sub);
  return Constructor;
}
(Base as any).extend = protoExtend;

const Products = (Base as any).extend({ create: makeMethod({ path: "/products" }) });
const Customers = (Base as any).extend({ create: makeMethod({ path: "/customers" }) });
const Webhooks = (Base as any).extend({ create: makeMethod({ path: "/webhooks" }) });

// Each resource constructor must own a DISTINCT prototype object.
console.log("Products.prototype === Webhooks.prototype:", Products.prototype === Webhooks.prototype); // false
console.log("Products.prototype.create === Webhooks.prototype.create:", Products.prototype.create === Webhooks.prototype.create); // false

// Instantiate through a ResourceNamespace-style loop, like Stripe does.
const resources: any = { products: Products, customers: Customers, webhooks: Webhooks };
function Namespace(this: any) {
  for (const name in resources) {
    this[name] = new resources[name]();
  }
}
const stripe: any = new (Namespace as any)();

// The crux: each method must dispatch to its OWN resource, not the last one.
console.log("products.create():", stripe.products.create()); // /products
console.log("customers.create():", stripe.customers.create()); // /customers
console.log("webhooks.create():", stripe.webhooks.create()); // /webhooks
