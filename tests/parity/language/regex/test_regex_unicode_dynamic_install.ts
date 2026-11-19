const name = process.argv[2] ?? "L";
const someString = "\\p{" + name + "}";
const constructed = new RegExp(someString, "u");
console.log(constructed.test("α"), constructed.test("1"));
const called = RegExp(someString, "u");
console.log(called.test("é"), called.test("-"));
