const sp = new URLSearchParams("a=1&b=2&a=3");
console.log("toString:", sp.toString());
console.log("size:", sp.size);
console.log("get:", sp.get("a"));
console.log("getAll:", sp.getAll("a").join(","));
console.log("has:", sp.has("b"));
