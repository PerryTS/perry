// #9239: a class prototype replacement applies to existing and fresh
// instances. The stored fresh receiver is important: it exercises Perry's
// exact-receiver method inliner instead of only the runtime dispatch path.
class C {
  m() { return "orig"; }
}

const old = new C();
console.log("before:", old.m());
(C.prototype as any).m = function () { return "replaced"; };
console.log("old:", old.m());

const fresh = new C();
console.log("fresh:", fresh.m());
console.log("direct:", new C().m());

const DynamicC: any = C;
console.log("dynamic:", new DynamicC().m());

class HelperC {
  m() { return "helper-orig"; }
}
function replaceThroughHelper() {
  (HelperC.prototype as any).m = function () { return "helper-replaced"; };
}
replaceThroughHelper();
const helperFresh = new HelperC();
console.log("helper:", helperFresh.m());
