const target: any = { name: "id", dataType: "string", notNull: true };
const proxy: any = new Proxy(target, { get(t: any, k: any) { return t[k]; } });
let acc = "";
for (let i = 0; i < 1000; i++) acc = `${proxy.name}:${proxy.dataType}:${proxy.notNull}`;
if (acc !== "id:string:true") throw new Error("proxy string read: " + acc);

const p2: any = new Proxy(target, {});
if (p2.name !== "id") throw new Error("proxy target forwarding");
if (proxy.absent !== undefined) throw new Error("absent key");

class Column { name = "col"; sql = "SELECT"; }
const p3: any = new Proxy(new Column(), { get(t: any, k: any) { return t[k]; } });
let result = "";
for (let i = 0; i < 1000; i++) result = `${p3.name}/${p3.sql}`;
if (result !== "col/SELECT") throw new Error("proxy class field: " + result);
console.log("OK");
