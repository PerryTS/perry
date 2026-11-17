// Pattern assembly ensures the constructor consumes a runtime string.
function property(name: string): RegExp {
  const pattern = "\\p{" + name + "}";
  return new RegExp(pattern, "u");
}
console.log(property("L").test("α"), property("L").test("1"));
console.log(/\P{L}/u.test("1"), /\P{L}/u.test("α"));
console.log(/[\p{L}&&\p{Script=Greek}]/v.test("α"));
console.log(/[\p{L}--\p{Script=Greek}]/v.test("α"));
console.log(/\p{Lowercase_Letter}/iu.test("A"));
console.log(/\P{Lowercase_Letter}/iv.test("A"));
console.log(/(?<α>a)\k<α>/u.test("aa"));
console.log(/(?<\u03b1>a)\k<\u03b1>/.test("aa"));
console.log(/k/iu.test("K"), /s/iu.test("ſ"));
console.log(/\p{RGI_Emoji}/v.test("👩‍💻"));
console.log("one two".split(/\s+/).join(","));
