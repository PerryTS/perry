const source = process.argv[2] || "return a * b";
console.log(new Function("a", "b", source)(6, 7));
