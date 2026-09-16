// @covers node:http2 settings()/localSettings/remoteSettings ACK ordering (#10327)
//
// ORACLE: Node 26.5.1. This is the fixture the loopback simulation cannot
// survive. Perry's `queue_session_settings` (perry-ext-http
// `server/http2_server/controls.rs`) applies the new settings to the caller
// IMMEDIATELY, pushes `remoteSettings` at whatever in-process session looks
// like a peer, and fires the user callback with `call2(callback, err,
// settings)` — two arguments. Node ties every one of those to a SETTINGS ACK
// arriving from the real peer, and passes THREE arguments.
//
// Established against Node, with a raw peer that ACKs only when told to:
//   * `pendingSettingsAck` is TRUE from the moment the session connects — the
//     initial SETTINGS frame is outstanding until the peer ACKs it;
//   * the FIRST ACK resolves the INITIAL settings: `'localSettings'` fires
//     with the connect-time values, not with anything the program asked for;
//   * `settings({...})` writes one SETTINGS frame whose records are in
//     ascending identifier order, and changes NOTHING observable yet:
//     `session.localSettings` still reports the old values and the callback
//     has not fired;
//   * only the SECOND ACK fires the callback, as
//     `(null, settings, duration)` — THREE arguments, the third a number > 0;
//   * `'localSettings'` fires once per ACK, in order, and
//     `session.localSettings` updates only then;
//   * a SETTINGS frame from the peer fires `'remoteSettings'` and Perry must
//     ACK it on the wire.
import http2 from "node:http2";
import {
  RawServerPeer,
  dump,
  sleep,
  frame,
  settingsPayload,
  FRAME_SETTINGS,
  FLAG_ACK,
  waitEvent,
} from "./_helpers/h2_wire.ts";

const peer = new RawServerPeer(undefined, false); // no auto-ACK: we drive it
const port = await peer.listen();
const client: any = http2.connect("http://127.0.0.1:" + port);

const events: string[] = [];
client.on("localSettings", (s: any) => {
  events.push("localSettings iws=" + s.initialWindowSize + " mcs=" + s.maxConcurrentStreams);
});
client.on("remoteSettings", (s: any) => {
  events.push("remoteSettings iws=" + s.initialWindowSize + " mcs=" + s.maxConcurrentStreams);
});
if (!(await waitEvent(client, "connect", 800))) console.log("!! client never emitted connect");
await sleep(150);

dump("wire after connect", peer.take());
console.log("events after connect:", JSON.stringify(events));
console.log("pendingSettingsAck at connect:", client.pendingSettingsAck);
events.length = 0;

peer.send(frame(FRAME_SETTINGS, FLAG_ACK, 0));
await sleep(150);
console.log("-- after ACK #1 (resolves the INITIAL settings) --");
console.log("events:", JSON.stringify(events));
console.log("pendingSettingsAck:", client.pendingSettingsAck);
console.log("localSettings.initialWindowSize:", client.localSettings.initialWindowSize);
events.length = 0;

let callback = "not-fired";
client.settings({ initialWindowSize: 32768, maxConcurrentStreams: 9 }, (err: any, s: any, duration: number) => {
  callback =
    "err=" + (err === null ? "null" : String(err && err.code)) +
    " iws=" + s.initialWindowSize +
    " mcs=" + s.maxConcurrentStreams +
    " durationIsNumber=" + (typeof duration === "number") +
    " durationPositive=" + (duration > 0);
});
await sleep(150);
console.log("-- after settings(), BEFORE ACK #2 --");
dump("wire", peer.take());
console.log("events:", JSON.stringify(events));
console.log("pendingSettingsAck:", client.pendingSettingsAck);
console.log("localSettings.initialWindowSize:", client.localSettings.initialWindowSize);
console.log("callback:", callback);

peer.send(frame(FRAME_SETTINGS, FLAG_ACK, 0));
await sleep(150);
console.log("-- after ACK #2 --");
console.log("events:", JSON.stringify(events));
console.log("pendingSettingsAck:", client.pendingSettingsAck);
console.log("localSettings.initialWindowSize:", client.localSettings.initialWindowSize);
console.log("localSettings.maxConcurrentStreams:", client.localSettings.maxConcurrentStreams);
console.log("callback:", callback);
events.length = 0;

// A SETTINGS frame from the peer must fire remoteSettings AND be ACKed on the
// wire by the session under test.
peer.send(frame(FRAME_SETTINGS, 0, 0, settingsPayload([[0x3, 11], [0x4, 4096]])));
await sleep(200);
console.log("-- peer sends SETTINGS mcs=11 iws=4096 --");
console.log("events:", JSON.stringify(events));
dump("wire", peer.take());
console.log("remoteSettings.maxConcurrentStreams:", client.remoteSettings.maxConcurrentStreams);

client.destroy();
peer.close();
