import crypto from "node:crypto";
// timingSafeEqual compares two equal-length buffers in constant time.
console.log("equal:", crypto.timingSafeEqual(Buffer.from("abc"), Buffer.from("abc")));
console.log("not equal:", crypto.timingSafeEqual(Buffer.from("abc"), Buffer.from("abd")));
