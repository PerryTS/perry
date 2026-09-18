// @covers node:http2 server SETTINGS on the wire (#10327 / http2 real transport)
//
// ORACLE: Node 26.5.1. A raw TCP peer speaks the HTTP/2 connection preface at
// an `http2.createServer()` and prints every frame the server emits. Nothing
// here can be satisfied by Perry's in-process loopback simulation
// (perry-ext-http `server/http2_server/controls.rs` scans the handle table for
// a peer `Http2SessionHandle`): the peer is a socket, so there is no second
// session handle to find, and a server that never encodes a SETTINGS frame
// prints "(none)".
//
// Established against Node:
//   * a default server's first frame is an EMPTY SETTINGS frame (length 0);
//   * the server ACKs the peer's SETTINGS with flags=0x1 and length 0;
//   * `createServer({ settings })` serialises identifiers in ASCENDING
//     identifier order (2=enablePush, 3=maxConcurrentStreams,
//     4=initialWindowSize), the same order as `getPackedSettings`;
//   * the server does NOT send a connection-level WINDOW_UPDATE up front.
import http2 from "node:http2";
import {
  RawClientPeer,
  dump,
  sleep,
  settingsPayload,
  barrier,
} from "./_helpers/h2_wire.ts";

async function withServer(
  label: string,
  options: any,
  clientSettings?: any,
): Promise<void> {
  const server = options === null ? http2.createServer() : http2.createServer(options);
  await new Promise<void>((r) => server.listen(0, "127.0.0.1", () => r()));
  const port = (server.address() as any).port;
  const peer = await RawClientPeer.connect(port, clientSettings);
  await sleep(250);
  dump(label, peer.take());
  peer.destroy();
  await barrier(new Promise<void>((r) => server.close(() => r())), 500);
}

await withServer("default server, peer sends empty SETTINGS", null);

await withServer("default server, peer advertises mcs=100 iws=65535", null,
  settingsPayload([[0x3, 100], [0x4, 65535]]));

await withServer(
  "server configured with { enablePush:false, maxConcurrentStreams:7, initialWindowSize:1234 }",
  { settings: { enablePush: false, maxConcurrentStreams: 7, initialWindowSize: 1234 } },
);

await withServer(
  "server configured with every settings key",
  {
    settings: {
      headerTableSize: 8192,
      enablePush: false,
      maxConcurrentStreams: 11,
      initialWindowSize: 131072,
      maxFrameSize: 32768,
      maxHeaderListSize: 40000,
    },
  },
);
