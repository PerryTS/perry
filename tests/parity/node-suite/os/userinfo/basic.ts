import * as os from "node:os";

const info = os.userInfo();
console.log("userinfo object:", info !== null && typeof info === "object");
console.log("userinfo not array:", !Array.isArray(info));
