const p = new URLSearchParams("b=2&a=1&b=3");
console.log("size before:", (p as any).size);
p.sort();
console.log("sorted:", p.toString());
console.log("entries:", Array.from(p.entries()).map(x => x.join(":" )).join(","));
