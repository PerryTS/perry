// P12 acceptance: the `ioredis` surface over **TLS**, run identically on Perry
// and on Node 26.5.1 with the real npm `ioredis`, against the same
// TLS-enabled Redis server.
//
// Redis has no in-band upgrade, so its TLS listens on its own port
// (`--tls-port`) and the session is installed the moment the socket connects,
// before a single RESP byte. That ordering is the point: `REDIS_PASSWORD`
// becomes an `AUTH` command in the handshake, and a client that sent it before
// the upgrade would have sent it in the clear.
//
// This is also the configuration Perry could not serve AT ALL until now:
// `REDIS_TLS` defaults to `true`, which built a `rediss://` URL, which declined
// to a legacy path whose `redis` dependency has no TLS backend compiled in. So
// the default `new Redis()` failed on every Perry build.
//
// The constructor argument is honoured by Node's ioredis and ignored by Perry's
// binding, which reads REDIS_HOST / REDIS_PORT / REDIS_TLS from the
// environment — one source file, the same server, provided both are set.
//
//   REDIS_HOST=127.0.0.1 REDIS_PORT=56380 REDIS_TLS=true
//   NODE_EXTRA_CA_CERTS=<ca.crt>   (Perry reads it too, through
//                                   perry_ffi::node_tls_client_environment)
//
// parity-skip: requires a live TLS-enabled Redis fixture
import Redis from "ioredis";
import { readFileSync } from "fs";

const HOST = process.env.REDIS_HOST ?? "127.0.0.1";
const PORT = Number(process.env.REDIS_PORT ?? "6380");
const ca = readFileSync(process.env.TLS_CA ?? "/dev/null", "utf8");

async function main(): Promise<void> {
  const redis = new Redis({ host: HOST, port: PORT, tls: { ca } });

  // Every key this run touches is cleared first, one call each. Silent, and
  // one key per call on purpose: a leftover `p12:counter` would make the two
  // arms print different numbers for a reason that says nothing about the
  // transport, and Perry's `del` has no variadic row in the compiler's
  // native-method table, so a multi-key call returns a different count on the
  // two engines — a pre-existing divergence this file must not assert.
  const key = "p12:redis:tls";
  await redis.del(key);
  await redis.del("p12:utf8");
  await redis.del("p12:big");
  await redis.del("p12:counter");

  console.log("set:", await redis.set(key, "hello"));
  console.log("get:", await redis.get(key));
  console.log("get-missing:", await redis.get("p12:absent"));
  console.log("exists:", await redis.exists(key));

  // A multi-byte value with an embedded newline: the RESP length prefix and the
  // UTF-8 decode now both sit under a record layer that can split them
  // anywhere, so neither may be assumed to arrive whole.
  const tricky = "héllo\nwörld";
  console.log("set-utf8:", await redis.set("p12:utf8", tricky));
  console.log("get-utf8:", JSON.stringify(await redis.get("p12:utf8")));

  // A value larger than one TLS record (16 KiB) and larger than one socket
  // read, so reassembly is exercised across both boundaries rather than
  // assumed. Only the length is printed.
  const big = "z".repeat(70000);
  await redis.set("p12:big", big);
  const back = await redis.get("p12:big");
  console.log("big-len:", back === null ? -1 : back.length);
  console.log("big-intact:", back === big);

  console.log("incr:", await redis.incr("p12:counter"));
  console.log("incr2:", await redis.incr("p12:counter"));
  console.log("del:", await redis.del(key));

  await redis.quit();
  console.log("done");
}

main().catch((e) => {
  console.log("FAILED:", e instanceof Error ? e.message : String(e));
  process.exitCode = 1;
});
