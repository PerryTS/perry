const sp = new URLSearchParams("a=1&b=2");
console.log("keys:", Array.from(sp.keys()).join(","));
console.log("values:", Array.from(sp.values()).join(","));
console.log("entries:", Array.from(sp.entries()).map(([k, v]) => `${k}=${v}`).join(","));

const collected: string[] = [];
sp.forEach((value, key) => { collected.push(`${key}=${value}`); });
console.log("forEach:", collected.join(","));

const iterated: string[] = [];
for (const [k, v] of sp) iterated.push(`${k}=${v}`);
console.log("for-of:", iterated.join(","));
