import { parse } from "node:querystring";

const parsed = parse("a%20b=c%20d&caf%C3%A9=ol%C3%A9&plus=a+b");
console.log("space key:", parsed["a b"]);
console.log("utf8 key:", parsed["café"]);
console.log("plus:", parsed.plus);
