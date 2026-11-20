// HWS companion: thread
// HWS hook: js_runtime_install_thread
// HWS prefixes: perry_runtime::thread::
const object={a:[1,2],b:{value:"ok"}};
const copy=structuredClone(object);copy.a[0]=9;
console.log(object.a[0],copy.a.join(","),copy.b.value);
const cycle:any={};cycle.self=cycle;const copied=structuredClone(cycle);console.log(copied===copied.self);
