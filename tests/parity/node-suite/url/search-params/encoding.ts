const sp = new URLSearchParams();
sp.append("a", "hello world");
sp.append("b", "1+2=3");
sp.append("c", "café");
sp.append("d", "&=?#");
console.log("toString:", sp.toString());
console.log("a:", sp.get("a"));
console.log("b:", sp.get("b"));
console.log("c:", sp.get("c"));
console.log("d:", sp.get("d"));

const decoded = new URLSearchParams("a=hello+world&b=1%2B2%3D3");
console.log("decoded a:", decoded.get("a"));
console.log("decoded b:", decoded.get("b"));
