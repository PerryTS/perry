import querystring from "node:querystring";

const input = "a=1;b=2;c=3%204";
console.log("default:", JSON.stringify(querystring.parse(input)));
console.log("semicolon:", JSON.stringify(querystring.parse(input, ";", "=")));
