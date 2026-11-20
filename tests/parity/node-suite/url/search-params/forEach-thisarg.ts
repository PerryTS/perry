const p = new URLSearchParams("a=1&b=2");
const ctx = { prefix: "ctx" };
const out: string[] = [];
p.forEach(function(this: any, value, key, obj) { out.push(this.prefix + ":" + key + "=" + value + ":" + (obj === p)); }, ctx);
console.log("out:", out.join(","));
