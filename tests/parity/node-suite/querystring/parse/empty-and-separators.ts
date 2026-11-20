import { parse } from "node:querystring";

function keyCount(value: object): number {
  return Object.keys(value).length;
}

console.log("empty keys:", keyCount(parse("")));
console.log("only separators keys:", keyCount(parse("&&&")));
console.log("extra separators:", JSON.stringify(parse("&&a=1&&b=2&")));
