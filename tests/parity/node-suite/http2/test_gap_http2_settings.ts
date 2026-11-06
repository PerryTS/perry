// @covers http2.getDefaultSettings / getPackedSettings / getUnpackedSettings (#3168)
// HTTP/2 SETTINGS pack/unpack: Node-shaped defaults, 6-byte big-endian
// records in identifier order, round-trip, and Node-compatible validation
// errors (RangeError / TypeError with ERR_HTTP2_* / ERR_INVALID_ARG_TYPE).
import http2 from "node:http2";

console.log("defaults:", JSON.stringify(http2.getDefaultSettings()));

const empty = http2.getPackedSettings({});
console.log("empty:", empty.length, Buffer.isBuffer(empty), empty.toString("hex"));

const packed = http2.getPackedSettings({
  headerTableSize: 4096,
  initialWindowSize: 65535,
  enablePush: false,
});
console.log("packed:", packed.length, Buffer.isBuffer(packed), packed.toString("hex"));

const full = http2.getPackedSettings({
  headerTableSize: 1,
  enablePush: true,
  maxConcurrentStreams: 100,
  initialWindowSize: 7,
  maxFrameSize: 16385,
  maxHeaderListSize: 9,
  enableConnectProtocol: true,
});
console.log("full:", full.length, full.toString("hex"));

console.log("unpacked:", JSON.stringify(http2.getUnpackedSettings(packed)));
console.log("unpacked-full:", JSON.stringify(http2.getUnpackedSettings(full)));

function caught(label: string, fn: () => void) {
  try {
    fn();
    console.log(label, "NO-THROW");
  } catch (e: any) {
    console.log(label, "|", e.name, "|", e.code, "|", e.message);
  }
}

caught("frame-low", () => http2.getPackedSettings({ maxFrameSize: 1 }));
caught("push-num", () => http2.getPackedSettings({ enablePush: 3 as any }));
caught("table-neg", () => http2.getPackedSettings({ headerTableSize: -1 }));
caught("win-big", () => http2.getPackedSettings({ initialWindowSize: 4294967296 }));
caught("unpack-len5", () => http2.getUnpackedSettings(Buffer.alloc(5)));
caught("unpack-string", () => http2.getUnpackedSettings("xxxxxx" as any));
caught("unpack-number", () => http2.getUnpackedSettings(6 as any));
caught("unpack-null", () => http2.getUnpackedSettings(null as any));
