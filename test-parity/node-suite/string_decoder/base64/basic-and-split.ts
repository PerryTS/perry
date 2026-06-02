import { StringDecoder } from "node:string_decoder";

const whole = new StringDecoder("base64");
console.log("whole:", whole.write(Buffer.from("hello")) + whole.end());

const split = new StringDecoder("base64");
console.log("split1:", JSON.stringify(split.write(Buffer.from([0x61]))));
console.log("splitEnd1:", JSON.stringify(split.end()));
console.log("split2:", JSON.stringify(split.write(Buffer.from([0x61, 0x61]))));
console.log("splitEnd2:", JSON.stringify(split.end()));
