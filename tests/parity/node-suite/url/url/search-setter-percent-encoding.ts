const u = new URL("https://example.com/a?old=1#h");
u.search = "x=1 2&y=a#b";
console.log("href:", u.href);
console.log("search:", u.search);
console.log("x:", u.searchParams.get("x"));
console.log("y:", u.searchParams.get("y"));

const encoded = new URL("https://example.com/");
encoded.search = "x=%20";
console.log("encoded href:", encoded.href);
console.log("encoded search:", encoded.search);
console.log("encoded x:", encoded.searchParams.get("x"));

const unicode = new URL("https://example.com/");
unicode.search = "x=\u00e9";
console.log("unicode href:", unicode.href);
console.log("unicode search:", unicode.search);
console.log("unicode x:", unicode.searchParams.get("x"));
