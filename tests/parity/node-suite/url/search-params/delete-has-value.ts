const p = new URLSearchParams("a=1&a=2&a=1&b=1");
console.log("has value:", p.has("a", "2"));
p.delete("a", "1");
console.log("after delete value:", p.toString());
console.log("getAll:", p.getAll("a").join(","));
