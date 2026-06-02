import { Buffer } from "node:buffer";

const b = Buffer.from("a😀b😀c");
console.log("index string:", b.indexOf("😀"));
console.log("last string:", b.lastIndexOf("😀"));
console.log("includes buffer:", b.includes(Buffer.from("b")));
console.log("ucs2:", Buffer.from("ab", "utf16le").indexOf("b", 0, "utf16le"));
console.log("nan offset:", b.indexOf("a", NaN as any));
