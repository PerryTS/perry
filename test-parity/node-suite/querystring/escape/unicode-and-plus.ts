import querystring from "node:querystring";

for (const value of ["a b", "a+b", "é", "😀", "!*'()"] ) {
  const escaped = querystring.escape(value);
  console.log("escape:", value, "=>", escaped, "=>", querystring.unescape(escaped));
}
