// Mirrors Stripe's compiled `resources/Products.js`: builds the resource via
// the cross-module `StripeResource.extend({ create: stripeMethod(...) })`.
import { StripeResource } from "./resource";
const stripeMethod = (StripeResource as any).method;

export const Products: any = (StripeResource as any).extend({
  create: stripeMethod({ method: "POST", fullPath: "/v1/products" }),
  retrieve: stripeMethod({ method: "GET", fullPath: "/v1/products/{id}" }),
});
