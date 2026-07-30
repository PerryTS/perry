import { constants } from "node:http2";

console.log("authority:", constants.HTTP2_HEADER_AUTHORITY);
console.log("method:", constants.HTTP2_HEADER_METHOD);
console.log("path:", constants.HTTP2_HEADER_PATH);
console.log("scheme:", constants.HTTP2_HEADER_SCHEME);
console.log("status:", constants.HTTP2_HEADER_STATUS);
