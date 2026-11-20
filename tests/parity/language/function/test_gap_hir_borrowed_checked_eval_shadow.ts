const code = process.argv[2] || "var Math = { max: function () { return 914; } };";
const run = new Function("code", "eval(code); return Math.max.call(null, 1, 2);");
console.log(run(code));
