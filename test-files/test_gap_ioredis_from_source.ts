// The npm `ioredis` package compiled from its own JS source (the native
// perry-ext-ioredis binding was removed). Exercises connect, SET/GET, a
// pipeline, MULTI/EXEC, SUBSCRIBE/PUBLISH and QUIT against an in-process
// fake RESP server, so no external redis-server is needed.
import Redis from "ioredis";
import { startFakeRedis } from "./_helpers/fake_resp_server.ts";
import { createRequire } from "node:module";

// Print the resolved package version so the output proves what ran.
const requireFromHere = createRequire(import.meta.url);
console.log("ioredis version:", requireFromHere("ioredis/package.json").version);

startFakeRedis(async (port, close) => {
  try {
    const r = new Redis({ host: "127.0.0.1", port });
    r.on("error", (e: any) => console.log("error:", e && e.message));
    console.log("set:", await r.set("k:a", "value-a"));
    console.log("get:", await r.get("k:a"));
    console.log("get missing:", await r.get("k:missing"));
    const pipe = await r.pipeline().set("k:n", "1").incr("k:n").get("k:n").exec();
    console.log("pipeline:", JSON.stringify(pipe));
    const multi = await r.multi().set("k:m", "2").incrby("k:m", 5).get("k:m").exec();
    console.log("multi:", JSON.stringify(multi));
    const sub = new Redis({ host: "127.0.0.1", port });
    const got = new Promise<string>((resolve) =>
      sub.on("message", (ch: string, msg: string) => resolve(ch + "=" + msg)));
    console.log("subscribe:", await sub.subscribe("k:chan"));
    console.log("publish:", await r.publish("k:chan", "hello"));
    console.log("message:", await got);
    console.log("del:", await r.del("k:a", "k:n", "k:m"));
    console.log("sub quit:", await sub.quit());
    console.log("quit:", await r.quit());
  } catch (e: any) {
    console.log("FAILED:", e && e.message);
  }
  close();
  console.log("done");
});
