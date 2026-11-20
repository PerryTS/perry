import { parse } from "node:querystring";

console.log("%G1:", JSON.stringify(parse("a=%G1")));
console.log("trailing %:", JSON.stringify(parse("a=%")));
console.log("short %2:", JSON.stringify(parse("a=%2")));
console.log("%ZZ:", JSON.stringify(parse("a=%ZZ")));
console.log("mixed:", JSON.stringify(parse("a=%41%G1%42")));
