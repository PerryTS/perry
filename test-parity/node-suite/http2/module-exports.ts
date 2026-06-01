import * as http2 from "node:http2";
import { constants, sensitiveHeaders } from "node:http2";

const moduleKeys = Object.keys(http2);
const constantKeys = Object.keys(constants);
const sensitiveDescriptor = Object.getOwnPropertyDescriptor(http2, "sensitiveHeaders");

console.log("module has constants:", moduleKeys.includes("constants"));
console.log("module has sensitiveHeaders:", moduleKeys.includes("sensitiveHeaders"));
console.log("sensitive typeof:", typeof http2.sensitiveHeaders);
console.log("sensitive import same:", sensitiveHeaders === http2.sensitiveHeaders);
console.log("sensitive stable:", http2.sensitiveHeaders === http2.sensitiveHeaders);
console.log("sensitive string:", String(http2.sensitiveHeaders));
console.log("sensitive description:", (http2.sensitiveHeaders as any).description);
console.log("sensitive keyFor:", String(Symbol.keyFor(http2.sensitiveHeaders)));
console.log(
  "sensitive descriptor:",
  sensitiveDescriptor?.enumerable,
  sensitiveDescriptor?.writable,
);
console.log("constants key count:", constantKeys.length);
console.log("constants first five:", constantKeys.slice(0, 5).join(","));
console.log("constants last five:", constantKeys.slice(-5).join(","));
console.log("DEFAULT_SETTINGS_MAX_CONCURRENT_STREAMS:", constants.DEFAULT_SETTINGS_MAX_CONCURRENT_STREAMS);
console.log("DEFAULT_SETTINGS_ENABLE_CONNECT_PROTOCOL:", constants.DEFAULT_SETTINGS_ENABLE_CONNECT_PROTOCOL);
console.log("MAX_MAX_FRAME_SIZE:", constants.MAX_MAX_FRAME_SIZE);
console.log("MIN_MAX_FRAME_SIZE:", constants.MIN_MAX_FRAME_SIZE);
console.log("MAX_INITIAL_WINDOW_SIZE:", constants.MAX_INITIAL_WINDOW_SIZE);
console.log("NGHTTP2_ERR_FRAME_SIZE_ERROR:", constants.NGHTTP2_ERR_FRAME_SIZE_ERROR);
console.log("NGHTTP2_STREAM_STATE_CLOSED:", constants.NGHTTP2_STREAM_STATE_CLOSED);
console.log("NGHTTP2_SETTINGS_ENABLE_CONNECT_PROTOCOL:", constants.NGHTTP2_SETTINGS_ENABLE_CONNECT_PROTOCOL);
console.log("NGHTTP2_DEFAULT_WEIGHT:", constants.NGHTTP2_DEFAULT_WEIGHT);
console.log("NGHTTP2_FLAG_PADDED:", constants.NGHTTP2_FLAG_PADDED);
console.log("PADDING_STRATEGY_CALLBACK:", constants.PADDING_STRATEGY_CALLBACK);
console.log("HTTP2_HEADER_TE:", constants.HTTP2_HEADER_TE);
console.log("HTTP2_HEADER_CONTENT_DISPOSITION:", constants.HTTP2_HEADER_CONTENT_DISPOSITION);
console.log("HTTP2_HEADER_ACCESS_CONTROL_ALLOW_ORIGIN:", constants.HTTP2_HEADER_ACCESS_CONTROL_ALLOW_ORIGIN);
console.log("HTTP2_HEADER_X_FORWARDED_FOR:", constants.HTTP2_HEADER_X_FORWARDED_FOR);
console.log("HTTP2_METHOD_MKCALENDAR:", constants.HTTP2_METHOD_MKCALENDAR);
console.log("HTTP2_METHOD_VERSION_CONTROL:", constants.HTTP2_METHOD_VERSION_CONTROL);
console.log("HTTP_STATUS_TOO_EARLY:", constants.HTTP_STATUS_TOO_EARLY);
console.log("HTTP_STATUS_UNAVAILABLE_FOR_LEGAL_REASONS:", constants.HTTP_STATUS_UNAVAILABLE_FOR_LEGAL_REASONS);
console.log("HTTP_STATUS_NETWORK_AUTHENTICATION_REQUIRED:", constants.HTTP_STATUS_NETWORK_AUTHENTICATION_REQUIRED);
