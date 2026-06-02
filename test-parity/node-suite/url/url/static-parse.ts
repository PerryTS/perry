const parsed = URL.parse("https://example.com/x");
console.log("href:", parsed === null ? null : parsed.href);
console.log("invalid:", URL.parse("not a url"));
