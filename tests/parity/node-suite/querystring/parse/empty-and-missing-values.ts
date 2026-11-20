import querystring from "node:querystring";

for (const input of ["", "a", "a=", "=b", "&&a=1&&", "a=1&a=2"]) {
  console.log("parse:", JSON.stringify(input), JSON.stringify(querystring.parse(input)));
}
