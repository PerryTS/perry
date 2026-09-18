// P7 acceptance: the `ioredis` surface, run identically on Perry and on
// Node 26.5.1 with the real npm `ioredis`, against the same server.
//
// Every value printed here crosses the binding: `SET` returns the string
// "OK", `GET` a string or `null`, `EXISTS`/`INCR`/`DECR`/`DEL`/`EXPIRE`
// numbers. Those are exactly the conversions that moved from the `redis`
// crate's `FromRedisValue` impls to an explicit reply shape, so a divergence
// in any of them shows up as a diff against the oracle.
//
// Only Perry's *reachable* ioredis surface is exercised. `setex`, `ping`,
// `hget`, `hset`, `hdel`, `hlen` and `hgetall` all exist as `js_ioredis_*`
// symbols in both bindings but have no row in the compiler's native-method
// table (`crates/perry-codegen/src/lower_call/native_table/databases.rs`), so
// calling them from TypeScript returns `undefined` on **both** transports.
// That is a pre-existing Perry defect, reproduced on the base commit, and
// including those calls here would make this file assert the defect rather
// than the migration.
//
// The constructor argument is honoured by Node's ioredis and ignored by
// Perry's binding, which reads REDIS_HOST / REDIS_PORT / REDIS_TLS from the
// environment — so one source file reaches the same server on both, provided
// the environment is set for both. `REDIS_TLS=false` is required on Perry:
// its default is `true`, which no Perry build can actually serve.
//
//   REDIS_HOST=127.0.0.1 REDIS_PORT=56379 REDIS_TLS=false
//
// parity-skip: requires a live Redis fixture
import Redis from "ioredis";

const HOST = process.env.REDIS_HOST ?? "127.0.0.1";
const PORT = Number(process.env.REDIS_PORT ?? "6379");

async function main(): Promise<void> {
  const redis = new Redis({ host: HOST, port: PORT });

  // Silent, so a leftover key from an earlier run cannot make the two arms
  // differ on a count that says nothing about the transport.
  const key = "p7:parity";
  await redis.del(key);

  console.log("set:", await redis.set(key, "hello"));
  console.log("get:", await redis.get(key));
  console.log("get-missing:", await redis.get("p7:absent"));
  console.log("exists:", await redis.exists(key));
  console.log("exists-missing:", await redis.exists("p7:absent"));

  // A value with a multi-byte character and an embedded newline, so the
  // length prefix and the UTF-8 decode are both exercised rather than assumed.
  const tricky = "héllo\nwörld";
  console.log("set-utf8:", await redis.set("p7:utf8", tricky));
  console.log("get-utf8-ok:", (await redis.get("p7:utf8")) === tricky);

  // 64 KiB, which spans several reads: the core has to reassemble a bulk
  // string that arrives in pieces. A transport that only ever saw one read
  // per reply would pass everything above and fail here.
  const big = "x".repeat(65536);
  console.log("set-big:", await redis.set("p7:big", big));
  const readBig = await redis.get("p7:big");
  console.log("get-big-len:", readBig === null ? -1 : readBig.length);
  console.log("get-big-ok:", readBig === big);

  const counter = "p7:counter";
  await redis.del(counter);
  console.log("incr:", await redis.incr(counter));
  console.log("incr2:", await redis.incr(counter));
  console.log("decr:", await redis.decr(counter));

  console.log("expire:", await redis.expire(key, 60));
  console.log("expire-missing:", await redis.expire("p7:absent", 60));

  // Several commands in flight at once on ONE connection. Under the old
  // transport each borrowed its own tokio blocking-pool thread; on turnloop
  // they are pipelined into one socket and answered in submission order —
  // which is what `concurrent:` checks, since a reordered reply would pair
  // `p7:c0`'s value with another command's promise.
  const many = await Promise.all([
    redis.set("p7:c0", "v0"),
    redis.set("p7:c1", "v1"),
    redis.set("p7:c2", "v2"),
    redis.get("p7:c0"),
    redis.incr(counter),
  ]);
  console.log("concurrent:", many.join("|"));
  console.log("after-pipeline:", await redis.get("p7:c2"), await redis.get(counter));

  console.log("cleanup:", await redis.del(key), await redis.del(counter));
  for (const k of ["p7:utf8", "p7:big", "p7:c0", "p7:c1", "p7:c2"]) await redis.del(k);

  console.log("quit:", await redis.quit());
  console.log("done");
}

main().catch((e) => {
  console.log("FAILED:", e instanceof Error ? e.message : String(e));
  process.exitCode = 1;
});
