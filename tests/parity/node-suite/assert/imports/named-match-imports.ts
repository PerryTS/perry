import { match, doesNotMatch } from "node:assert";

match("abc", /b/);
doesNotMatch("abc", /z/);
console.log("named match imports ok");
