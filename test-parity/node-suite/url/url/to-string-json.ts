const u = new URL("https://example.com/p?q=1#h");
console.log("toString:", u.toString());
console.log("toJSON:", u.toJSON());
console.log("JSON.stringify:", JSON.stringify(u));
console.log("toString === href:", u.toString() === u.href);
console.log("toJSON === href:", u.toJSON() === u.href);
