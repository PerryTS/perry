function StripeResource(this: any, stripe: any, deprecated: any) {
  if (deprecated) throw new Error("invoked with a second arg");
  this._stripe = stripe;
}
(StripeResource as any).method = function () { return "M"; };
(StripeResource as any).extend = function () { return "E"; };
(StripeResource as any).MAX = 42;
(StripeResource as any).prototype = { _stripe: null, path: "", initialize() {} };
export { StripeResource };
