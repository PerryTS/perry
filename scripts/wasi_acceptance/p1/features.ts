// The optional features a program selects (regular expressions, URLs and
// runtime-compiled functions) link and run on the core-module target.
const m = /(\d{4})-(\d{2})-(\d{2})/.exec("on 2024-03-15 we shipped");
console.log(m ? `${m[3]}/${m[2]}/${m[1]}` : "no match");
console.log("a1b22c333".replace(/\d+/g, (d) => `<${d.length}>`));
const url = new URL("https://user@example.com:8443/a/b?x=1&y=two#frag");
console.log(url.hostname, url.port, url.pathname, url.searchParams.get("y"), url.hash);
const add = new Function("a", "b", "return a + b");
console.log(add(2, 3));
