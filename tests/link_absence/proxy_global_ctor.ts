// HWS companion: proxy
// HWS hook: js_runtime_install_proxy
// HWS prefixes: perry_runtime::proxy::|js_proxy_
// HWS argv: ["Proxy"]
const host: any = globalThis;
const C: any = host[process.argv[2]];
const p: any = new C({x: 9}, {get(t, k) { return Reflect.get(t, k) + 1; }});
console.log(p.x, typeof p);
