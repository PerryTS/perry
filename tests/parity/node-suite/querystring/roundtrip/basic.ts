import { parse, stringify } from "node:querystring";

const input = { a: "1", b: "hello world", café: "olé" };
const encoded = stringify(input);
const parsed = parse(encoded);
console.log("encoded:", encoded);
console.log("a:", parsed.a);
console.log("b:", parsed.b);
// Keep the label ASCII: this case is about querystring round-tripping UTF-8
// keys/values, not direct non-ASCII string-literal console output.
console.log("cafe:", parsed["café"]);
