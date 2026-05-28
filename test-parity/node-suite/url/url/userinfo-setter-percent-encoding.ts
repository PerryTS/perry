const u = new URL("https://example.com/a?x=1");
u.username = "u s";
u.password = "p@";
console.log("href:", u.href);
console.log("username:", u.username);
console.log("password:", u.password);

const encoded = new URL("https://example.com/");
encoded.username = "a%20b";
encoded.password = "c%40d";
console.log("encoded href:", encoded.href);
console.log("encoded username:", encoded.username);
console.log("encoded password:", encoded.password);

const unicode = new URL("https://example.com/");
unicode.username = "\u00e9";
unicode.password = "\u03c0";
console.log("unicode href:", unicode.href);
console.log("unicode username:", unicode.username);
console.log("unicode password:", unicode.password);

const fileUrl = new URL("file:///tmp/a");
fileUrl.username = "u s";
fileUrl.password = "p@";
console.log("file href:", fileUrl.href);
console.log("file username:", fileUrl.username);
console.log("file password:", fileUrl.password);
