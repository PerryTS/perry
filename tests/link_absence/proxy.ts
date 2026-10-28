// HWS hook: js_runtime_install_proxy
// HWS prefixes: perry_runtime::proxy::|js_proxy_
const p=new Proxy({x:7},{}); console.log(p.x);
