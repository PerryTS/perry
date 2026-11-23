// Code that names the global object's keys only at run time: listing them
// sees Navigator (perry has not born its family yet), and evaluated code
// reads Navigator.prototype with node's keys.
const names: string[] = new Function("return Object.getOwnPropertyNames(globalThis)")();
const out: string[] = [];
out.push("listed: " + String(names.includes("Navigator")) + "," + String(names.includes("navigator")));
const desc = Object.getOwnPropertyDescriptor(globalThis, "Navigator")!;
out.push("binding: " + [typeof desc.value, desc.writable, desc.enumerable, desc.configurable].join(","));
const P: any = eval("Navigator.prototype");
out.push("eval prototype: " + JSON.stringify(Object.getOwnPropertyNames(P)));
out.push("same: " + String(P === (globalThis as any).Navigator.prototype));
out.push("evaluated instance: " + String(new Function("return Object.getPrototypeOf(navigator) === Navigator.prototype")()));
console.log(out.join("\n"));
