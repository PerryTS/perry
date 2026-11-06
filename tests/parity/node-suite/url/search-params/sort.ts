const sp = new URLSearchParams("c=3&a=1&b=2&a=0");
sp.sort();
console.log("sorted:", sp.toString());
