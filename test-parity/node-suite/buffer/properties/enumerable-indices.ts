import { Buffer } from "node:buffer";

const b = Buffer.from([5, 6]);
console.log("has 0:", b.hasOwnProperty("0"));
console.log("has 2:", b.hasOwnProperty("2"));
console.log("enum 1:", b.propertyIsEnumerable("1"));
console.log("has num 1:", b.hasOwnProperty(1));
console.log("has leading:", b.hasOwnProperty("01"));
console.log("object has 0:", Object.hasOwn(b, "0"));
console.log("object has 2:", Object.hasOwn(b, "2"));
console.log("proto has 1:", Object.prototype.hasOwnProperty.call(b, "1"));
console.log("proto has 2:", Object.prototype.hasOwnProperty.call(b, "2"));
console.log("proto enum 0:", Object.prototype.propertyIsEnumerable.call(b, "0"));
console.log("proto enum 2:", Object.prototype.propertyIsEnumerable.call(b, "2"));
console.log("proto enum leading:", Object.prototype.propertyIsEnumerable.call(b, "01"));
console.log("proto enum length:", Object.prototype.propertyIsEnumerable.call(b, "length"));
console.log(
  "empty proto enum 0:",
  Object.prototype.propertyIsEnumerable.call(Buffer.alloc(0), "0"),
);
