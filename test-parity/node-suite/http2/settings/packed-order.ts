import { getPackedSettings } from "node:http2";

const packed = getPackedSettings({
  enableConnectProtocol: false,
  maxFrameSize: 16384,
  initialWindowSize: 7,
  maxConcurrentStreams: 6,
  enablePush: true,
  headerTableSize: 5,
});
console.log(packed.toString("hex"));
