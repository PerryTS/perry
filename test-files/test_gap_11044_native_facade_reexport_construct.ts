// #11044: `export { X } from "<node-core-builtin>"` (crypto, path, ...) was
// fixed by #10432/#10802/#10867 -- a local facade module re-exporting a
// builtin's named value now publishes a live getter instead of link-failing
// on a nonexistent local function body. This is the same defect one layer
// out: a facade re-exporting a named binding from a Perry-native NPM
// package that is not a Node builtin (here `ws`; the same shape applies to
// `ioredis`, `mysql2`, ...). ethers' `src.ts/providers/ws.ts` is exactly
// `export { WebSocket } from "ws";`, imported (renamed) by
// `provider-websocket.ts` and referenced inside a closure -- which used to
// undefined-reference-fail the link on
// `__perry_wrap_perry_fn_..._ws_ts__WebSocket`.
//
// This program never opens a socket: constructing `Connector` only captures
// the reference-taking closure, matching the original ethers repro, which
// reaches the link failure from the closure alone (nothing calls `.open()`).
// Node cannot run this file (`ws` is a real npm package, not vendored here,
// and no node core module aliases it) -- same as the pre-existing sibling
// `test_gap_turnloop_ws_client.ts`, which hits the identical
// `ERR_MODULE_NOT_FOUND` under `node --experimental-strip-types`. Both are
// Perry-only correctness checks: real npm-`ws` behavior is covered by the
// hand-rolled-server gap tests instead. What this file exercises -- and
// what regressed to a link failure -- is purely the facade's cross-module
// symbol resolution, so a deterministic non-network assertion is the right
// shape here.
import { WebSocket as _WebSocket } from "./_helpers/gap_11044_ws_reexport/ws_facade.ts";

class Connector {
  #connect: () => any;
  constructor(url: string) {
    this.#connect = () => {
      return new _WebSocket(url);
    };
  }
  hasConnector(): boolean {
    return typeof this.#connect === "function";
  }
}

const conn = new Connector("ws://127.0.0.1:1/");
console.log("hasConnector:", conn.hasConnector());
