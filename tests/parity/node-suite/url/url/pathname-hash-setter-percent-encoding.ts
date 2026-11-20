const u = new URL("https://example.com/a?x=1#old");
u.pathname = "/c d";
u.hash = "frag ment";
console.log("href:", u.href);
console.log("pathname:", u.pathname);
console.log("hash:", u.hash);

const encoded = new URL("https://example.com/");
encoded.pathname = "/a%20b";
encoded.hash = "x%20y";
console.log("encoded href:", encoded.href);
console.log("encoded pathname:", encoded.pathname);
console.log("encoded hash:", encoded.hash);

const unicode = new URL("https://example.com/");
unicode.pathname = "/\u00e9";
unicode.hash = "\u03c0";
console.log("unicode href:", unicode.href);
console.log("unicode pathname:", unicode.pathname);
console.log("unicode hash:", unicode.hash);
