// Issue #650 (partial): URL.canParse(s) and URL.parse(s) static methods.

console.log("--- canParse ---");
console.log(URL.canParse("https://example.com"));      // true
console.log(URL.canParse("https://user:pass@host:8080/p?q#h"));  // true
console.log(URL.canParse("file:///etc/hosts"));        // true
console.log(URL.canParse("mailto:a@b"));               // true
console.log(URL.canParse(""));                          // false
console.log(URL.canParse("not a url"));                 // false
console.log(URL.canParse("/relative"));                 // false
console.log(URL.canParse("https://"));                  // false (scheme but no authority)

console.log("--- parse ---");
const u1 = URL.parse("https://example.com/p");
console.log(u1?.href);                                   // "https://example.com/p"
console.log(u1?.host);                                   // "example.com"

const u2 = URL.parse("not a url");
console.log(u2);                                         // null
console.log(u2 === null);                                // true

const u3 = URL.parse("https://user:pass@example.com:8080/p?q=1#h");
console.log(u3?.href);
console.log(u3?.username);
console.log(u3?.password);
console.log(u3?.hash);

console.log("--- toString / toJSON ---");
const u4 = new URL("https://user:pass@example.com:8080/p/q?k=v#frag");
console.log(u4.toString());
console.log(u4.toJSON());
// Inline-ctor case (no intermediate let).
console.log(new URL("https://example.com/x").toString());
// Date.toJSON / toISOString still work — Date arm gate must allow them.
console.log(new Date(0).toJSON());
console.log(new Date(0).toISOString());

console.log("--- URLSearchParams.size ---");
const sp = new URLSearchParams("a=1&b=2&c=3");
console.log(sp.size);
sp.append("d", "4");
console.log(sp.size);
sp.delete("a");
console.log(sp.size);
console.log(new URLSearchParams("").size);
// Chained URL.searchParams.size (no intermediate let).
console.log(new URL("https://example.com/?x=1&y=2").searchParams.size);
