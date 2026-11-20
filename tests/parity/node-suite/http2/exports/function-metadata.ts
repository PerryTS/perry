import * as http2 from "node:http2";

for (
  const key of [
    "connect",
    "createServer",
    "createSecureServer",
    "getDefaultSettings",
    "getPackedSettings",
    "getUnpackedSettings",
    "performServerHandshake",
  ] as const
) {
  const value = http2[key];
  console.log(key, typeof value, value?.name, value?.length);
}
