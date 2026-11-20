import querystring from "node:querystring";

const parsedNumber = querystring.parse("a=1", undefined, undefined, {
  decodeURIComponent(): any {
    return 123;
  },
});
console.log("parse number keys:", Object.keys(parsedNumber).join(","));
console.log("parse number value:", typeof parsedNumber["123"], parsedNumber["123"]);

const parsedBool = querystring.parse("a=1", undefined, undefined, {
  decodeURIComponent(value: string): any {
    return value === "a" ? true : false;
  },
});
console.log("parse bool keys:", Object.keys(parsedBool).join(","));
console.log("parse bool value:", typeof parsedBool.true, parsedBool.true);

console.log(
  "stringify number:",
  querystring.stringify({ a: "x" }, undefined, undefined, {
    encodeURIComponent(): any {
      return 123;
    },
  }),
);
console.log(
  "stringify bool:",
  querystring.stringify({ a: "x" }, undefined, undefined, {
    encodeURIComponent(value: string): any {
      return value === "a" ? true : false;
    },
  }),
);
