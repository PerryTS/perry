const sp = new URLSearchParams("a=1&b=2&a=3");
sp.set("a", "only");
console.log("after set:", sp.toString());
sp.append("a", "again");
console.log("after append:", sp.toString());
sp.delete("b");
console.log("after delete:", sp.toString());
