console.log("empty:", new URLSearchParams().toString());
console.log("record:", new URLSearchParams({ a: "1", b: "2" }).toString());
console.log("iter:", new URLSearchParams([["a", "1"], ["b", "2"]]).toString());
console.log("copy:", new URLSearchParams(new URLSearchParams("a=1")).toString());
