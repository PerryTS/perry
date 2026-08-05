// The generic value paths (`ToString`, `+` coercion, `JSON.stringify`,
// `Object.fromEntries`, and the by-name field setter) each carry a runtime
// "is this object URL-shaped?" probe. Those probes are compiled only under
// `url-engine`, because their static references otherwise pin the whole URL
// parser into every binary. This file uses the URL API, so `uses_url` turns
// the feature on and every probe below must behave exactly as before.

const u = new URL("https://user:pw@example.com:8443/a/b?x=1&y=2#frag");
console.log(String(u));
console.log("" + u);
console.log(u.hostname, u.port, u.pathname, u.search, u.hash);
u.pathname = "/changed";
u.search = "?z=9";
u.href = "https://other.example/zzz?q=1";
console.log(u.href, u.hostname);
const sp = new URLSearchParams("a=1&b=2");
console.log(String(sp), "" + sp);
console.log(JSON.stringify({ u, sp: String(sp) }));
console.log(JSON.stringify(Object.fromEntries(sp)));
