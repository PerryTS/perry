const u = new URL("https://example.com/p?a=1");
u.searchParams.append("b", "2");
u.searchParams.set("a", "9");
console.log("href:", u.href);
console.log("search:", u.search);
console.log("a:", u.searchParams.get("a"));

u.search = "x=1&y=2";
console.log("after assign search href:", u.href);
console.log("searchParams x:", u.searchParams.get("x"));
console.log("searchParams y:", u.searchParams.get("y"));

u.searchParams.delete("x");
console.log("after delete x search:", u.search);
