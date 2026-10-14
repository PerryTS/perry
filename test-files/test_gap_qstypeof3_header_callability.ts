// Callable Class/Proxy facts and heap fields stay intact over loop collections.
function heapClass(n: number): any {
  const payload = { text: "class-heap-field-" + n, count: n };
  return class { static heap = payload; };
}
let heldClass: any = heapClass(0);
let heldProxy: any = new Proxy(heldClass, {});
let sum = 0;
for (let n = 1; n <= 24; n++) {
  const plain: any = { number: n };
  if (typeof plain !== "object") throw new Error("pointer-free object became callable");
  if (typeof heldClass !== "function" || typeof heldProxy !== "function")
    throw new Error("class/proxy lost callability");
  if (heldClass.heap.text !== "class-heap-field-" + (n - 1))
    throw new Error("class heap field changed");
  if (heldProxy.heap.count !== n - 1) throw new Error("proxy heap field changed");
  sum += heldClass.heap.count + heldProxy.heap.count;
  heldClass = heapClass(n);
  heldProxy = new Proxy(heldClass, {});
}
console.log("class-proxy-heap", sum, heldClass.heap.text, heldProxy.heap.count);

function check(label: string, value: any): void {
  console.log(label, typeof value, typeof value === "function", typeof value === "object");
}
function makeClass(n: number): any { return class { static captured = n; }; }
function regular() {}
async function asyncFn() {}
function* generator() { yield 1; }
async function* asyncGenerator() { yield 1; }
const C: any = makeClass(7);
check("class", C);
for (let i = 0; i < 4; i++) {
  const later: any = makeClass(i);
  check("template-class-" + i, later);
  check("proxy-template-class-" + i, new Proxy(later, {}));
}
check("bound", regular.bind(null));
check("async", asyncFn);
check("generator", generator);
check("async-generator", asyncGenerator);
check("native", Math.max);
check("native-bound", Object.prototype.hasOwnProperty.bind({}));
check("function-prototype", Function.prototype);
check("proxy-function-prototype", new Proxy(Function.prototype, {}));
check("proxy-class", new Proxy(C, {}));
check("proxy-function", new Proxy(regular, {}));
check("proxy-object", new Proxy({}, {}));
const revokedFn = Proxy.revocable(new Proxy(regular, {}), {});
const revokedObj = Proxy.revocable({}, {});
revokedFn.revoke(); revokedObj.revoke();
check("revoked-function", revokedFn.proxy);
check("revoked-object", revokedObj.proxy);
check("nested-revoked", new Proxy(revokedFn.proxy, {}));
const plain: any = Object.create(null);
check("null-proto", plain);
for (let i = 0; i < 1200; i++) {
  plain["key" + i] = i;
  C["key" + i] = i;
}
Object.defineProperty(C, "accessor", { get() { return 1; }, configurable: true });
delete C.key1;
Object.setPrototypeOf(C, null);
Object.freeze(C);
check("mutated-class", C);
check("mutated-object", plain);
check("call-trap-object", new Proxy({}, { apply() { return 1; } }));
check("symbol", Symbol("s"));
check("null", null);
const fp: any = Function.prototype;
fp.extra = 1;
delete fp.extra;
Object.setPrototypeOf(fp, null);
Object.freeze(fp);
check("mutated-function-prototype", fp);
check("proxy-mutated-function-prototype", new Proxy(fp, {}));
