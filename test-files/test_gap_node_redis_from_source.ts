// The npm `redis` package (node-redis) compiled from its own JS source
// (the native perry-ext-ioredis binding, which also served `redis`, was
// removed). Exercises connect, SET/GET, a pipeline, MULTI/EXEC,
// SUBSCRIBE/PUBLISH on a duplicate() client and QUIT against an in-process
// fake RESP server, so no external redis-server is needed.
import { createClient } from "redis";
import { startFakeRedis } from "./_helpers/fake_resp_server.ts";
import { createRequire } from "node:module";

// Print the resolved package version so the output proves what ran.
const requireFromHere = createRequire(import.meta.url);
console.log("redis version:", requireFromHere("redis/package.json").version);

startFakeRedis(async (port, close) => {
  try {
    // RESP: 2 — the fake server speaks RESP2 only (node-redis 6 defaults to a
    // RESP3 `HELLO 3` handshake; the live-server probe covers that default).
    const client = createClient({ RESP: 2, socket: { host: "127.0.0.1", port } });
    client.on("error", (e: any) => console.log("error:", e && e.message));
    await client.connect();
    console.log("connected:", client.isReady);
    console.log("set:", await client.set("r:a", "value-a"));
    console.log("get:", await client.get("r:a"));
    console.log("get missing:", await client.get("r:missing"));
    const pipe = await client.multi().set("r:n", "1").incr("r:n").get("r:n").execAsPipeline();
    console.log("pipeline:", JSON.stringify(pipe));
    const multi = await client.multi().set("r:m", "2").incrBy("r:m", 5).get("r:m").exec();
    console.log("multi:", JSON.stringify(multi));
    const sub = client.duplicate();
    sub.on("error", (e: any) => console.log("sub error:", e && e.message));
    await sub.connect();
    const got = new Promise<string>((resolve) => {
      sub.subscribe("r:chan", (msg: string, ch: string) => resolve(ch + "=" + msg));
    });
    await new Promise((r) => setTimeout(r, 50));
    console.log("publish:", await client.publish("r:chan", "hello"));
    console.log("message:", await got);
    console.log("del:", await client.del(["r:a", "r:n", "r:m"]));
    await sub.quit();
    await client.quit();
    console.log("quit: ok");
  } catch (e: any) {
    console.log("FAILED:", e && e.message);
  }
  close();
  console.log("done");
});
