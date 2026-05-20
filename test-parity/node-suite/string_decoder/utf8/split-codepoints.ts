import { StringDecoder } from "node:string_decoder";

const dec = new StringDecoder("utf8");
console.log("euro part1:", JSON.stringify(dec.write(Buffer.from([0xe2, 0x82]))));
console.log("euro part2:", JSON.stringify(dec.write(Buffer.from([0xac]))));
console.log("euro end:", JSON.stringify(dec.end()));

const emoji = new StringDecoder("utf8");
console.log("emoji 1:", JSON.stringify(emoji.write(Buffer.from([0xf0, 0x9f]))));
console.log("emoji 2:", JSON.stringify(emoji.write(Buffer.from([0x98]))));
console.log("emoji 3:", JSON.stringify(emoji.write(Buffer.from([0x80]))));
console.log("emoji end:", JSON.stringify(emoji.end()));
