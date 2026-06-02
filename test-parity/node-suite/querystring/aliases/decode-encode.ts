import * as querystring from "node:querystring";

console.log("decode parse same:", querystring.decode === querystring.parse);
console.log("encode stringify same:", querystring.encode === querystring.stringify);
console.log("decode:", JSON.stringify(querystring.decode("x=1")));
console.log("encode:", querystring.encode({ x: "1" }));
