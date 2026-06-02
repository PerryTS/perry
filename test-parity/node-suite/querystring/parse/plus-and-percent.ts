import querystring from "node:querystring";

for (const input of ["a+b=c+d", "a%2Bb=c%2Bd", "a=%E0%A4%A", "a=%F0%9F%98%80"]) {
  console.log("parse:", input, JSON.stringify(querystring.parse(input)));
}
