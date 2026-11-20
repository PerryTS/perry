// HWS companion: proxy
// HWS hook: js_runtime_install_proxy
// HWS prefixes: perry_runtime::proxy::|js_proxy_
const target: any = {x: 2};
const log: string[] = [];
const p: any = new Proxy(target, {
  get(t, k, r) { log.push("get:" + String(k)); return Reflect.get(t, k, r); },
  set(t, k, v, r) { log.push("set:" + String(k)); return Reflect.set(t, k, v, r); },
  defineProperty(t, k, d) { log.push("define:" + String(k)); return Reflect.defineProperty(t, k, d); },
  getOwnPropertyDescriptor(t, k) { log.push("desc:" + String(k)); return Reflect.getOwnPropertyDescriptor(t, k); },
  has(t, k) { log.push("has:" + String(k)); return Reflect.has(t, k); },
  deleteProperty(t, k) { log.push("delete:" + String(k)); return Reflect.deleteProperty(t, k); },
  ownKeys(t) { log.push("keys"); return Reflect.ownKeys(t); }
});
p.x = 7; console.log(p.x, "x" in p, Object.keys(p).join(","));
console.log(delete p.x, target.x, log.join("|"));
const f: any = new Proxy(function add(x) { return x + 1; }, {apply(t, self, args) { return Reflect.apply(t, self, args) * 2; }});
console.log(typeof f, f(4));
const pair = Proxy.revocable({value: 11}, {});
console.log(pair.proxy.value); pair.revoke();
try { console.log(pair.proxy.value); } catch (e) { console.log(e instanceof TypeError); }
