const u = new URL("https://example.com/a?x=1#old");
u.pathname = "changed";
u.search = "y=2";
u.hash = "new";
console.log("href:", u.href);
console.log("pathname:", u.pathname);
console.log("search:", u.search);
console.log("hash:", u.hash);
