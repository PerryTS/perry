import { constants } from "node:http2";

for (
  const key of [
    "NGHTTP2_NO_ERROR",
    "NGHTTP2_CANCEL",
    "NGHTTP2_SETTINGS_ENABLE_CONNECT_PROTOCOL",
    "DEFAULT_SETTINGS_INITIAL_WINDOW_SIZE",
    "MIN_MAX_FRAME_SIZE",
    "MAX_MAX_FRAME_SIZE",
    "HTTP2_HEADER_METHOD",
    "HTTP2_HEADER_STATUS",
    "HTTP2_METHOD_CONNECT",
    "HTTP_STATUS_OK",
  ] as const
) {
  console.log(key, constants[key]);
}
