import { unescape } from "node:querystring";

console.log("latin1:", unescape("caf%C3%A9"));
console.log("emoji:", unescape("%F0%9F%99%82"));
console.log("mixed:", unescape("a%20b%2Fcaf%C3%A9"));
