const sp = new URLSearchParams("a=1&a=2&b=3");
console.log("has a:", sp.has("a"));
console.log("has a=1:", sp.has("a", "1"));
console.log("has a=9:", sp.has("a", "9"));

sp.delete("a", "1");
console.log("after delete a=1:", sp.toString());

sp.delete("a");
console.log("after delete a:", sp.toString());
