import { Buffer } from "node:buffer";

class MyBuffer extends Buffer {}
const b: any = MyBuffer.from("abc");
console.log("instance my:", b instanceof MyBuffer);
console.log("instance buffer:", b instanceof Buffer);
console.log("slice ctor:", b.slice(0, 1).constructor.name);
