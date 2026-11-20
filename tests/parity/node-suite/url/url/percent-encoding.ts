const u = new URL("https://example.com/p%20ath?q=%20a&b=c%26d#h%20i");
console.log("pathname:", u.pathname);
console.log("search:", u.search);
console.log("hash:", u.hash);
console.log("q:", u.searchParams.get("q"));
console.log("b:", u.searchParams.get("b"));

const unicode = new URL("https://example.com/café/?q=á");
console.log("unicode pathname:", unicode.pathname);
console.log("unicode search:", unicode.search);
