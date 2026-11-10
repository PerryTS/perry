// Mirrors Stripe's compiled `StripeResource.js` + `utils.protoExtend` +
// `StripeMethod.stripeMethod`: a plain constructor FUNCTION whose `.prototype`
// is reassigned to an object literal, a Backbone-style `extend` factory, and a
// `stripeMethod` closure factory. Defined in its OWN module so the consumer
// reaches `extend`/`method` cross-module (this is the shape that broke #4831).
export function stripeMethod(spec: any) {
  return function (this: any, ...args: any[]) {
    return "called:" + spec.method + " path:" + this.path;
  };
}

function protoExtend(this: any, sub: any) {
  const Super: any = this;
  const Constructor: any = Object.prototype.hasOwnProperty.call(sub, "constructor")
    ? sub.constructor
    : function (this: any, ...args: any[]) {
        Super.apply(this, args);
      };
  Object.assign(Constructor, Super);
  Constructor.prototype = Object.create(Super.prototype);
  Object.assign(Constructor.prototype, sub);
  return Constructor;
}

export function StripeResource(this: any, stripe: any) {
  this._stripe = stripe;
  this.path = "init-path";
  this.initialize();
}
(StripeResource as any).extend = protoExtend;
(StripeResource as any).method = stripeMethod;
(StripeResource as any).prototype = {
  _stripe: null,
  path: "",
  initialize() {},
  _makeRequest() {
    return "req";
  },
};
