console.log("absolute:", URL.canParse("https://example.com"));
console.log("invalid:", URL.canParse("not a url"));
console.log("with base:", URL.canParse("/p", "https://example.com"));
