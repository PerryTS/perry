import { StringDecoder } from "node:string_decoder";

const basic = new StringDecoder("utf16le");
console.log("basic:", JSON.stringify(basic.write(Buffer.from([0x68, 0x00, 0x69, 0x00])) + basic.end()));

const split = new StringDecoder("utf16le");
console.log("split1:", JSON.stringify(split.write(Buffer.from("3DD8", "hex"))));
console.log("split2:", JSON.stringify(split.write(Buffer.from("4D", "hex"))));
console.log("split3:", JSON.stringify(split.write(Buffer.from("DC", "hex"))));
console.log("splitEnd:", JSON.stringify(split.end()));
