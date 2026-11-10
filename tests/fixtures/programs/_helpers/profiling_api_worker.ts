import { parentPort } from "node:worker_threads";
import { sampleTiming } from "./profiling_api_helper.ts";
const before = process.memoryUsage();
const bytes = new ArrayBuffer(4 * 1024 * 1024);
const view = new Uint8Array(bytes);
view[0] = 17;
const after = process.memoryUsage();
parentPort!.postMessage({ timing: sampleTiming(), memory: after.arrayBuffers - before.arrayBuffers >= bytes.byteLength && after.external >= after.arrayBuffers, used: after.heapUsed, total: after.heapTotal, byte: view[0] });
