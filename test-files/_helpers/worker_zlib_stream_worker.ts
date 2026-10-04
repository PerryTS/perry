import { parentPort } from "node:worker_threads";
import { inflatedBytes, payload } from "./worker_zlib_stream_lib.ts";

parentPort!.on("message", async (round: number) => {
    parentPort!.postMessage({ round, bytes: await inflatedBytes(payload(round + 3)) });
});
