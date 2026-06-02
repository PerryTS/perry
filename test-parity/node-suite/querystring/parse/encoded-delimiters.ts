import { parse } from "node:querystring";

const parsed = parse("a%3Db=c%26d&x%26y=1%3D2");
console.log("a=b:", parsed["a=b"]);
console.log("x&y:", parsed["x&y"]);
