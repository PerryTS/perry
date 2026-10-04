// parity-env: PERRY_GC_SCHEDULE_SEED=3 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=1
// GC witness for the message clone. The reader allocates every object of a
// message while the ones it made before must stay reachable for later
// references (shared objects, cycles, Map keys, the ArrayBuffer behind a
// second view). The writer builds a lazy JSON.parse array and an Error's
// stack text between two walks. With a collection at every allocation a
// missed root shows up as a wrong value or a crash.
import { Worker, MessageChannel } from "node:worker_threads";
import { describe } from "./_helpers/clone_describe.ts";

const guard = setTimeout(() => {
    console.log("timeout");
    process.exit(2);
}, 60000);

function graph(n: number): any {
    const shared = { tag: "shared", list: ["a", "b"] };
    const items: any[] = [];
    for (let i = 0; i < n; i++) {
        items.push({ id: "item-" + i, n: i, deps: { ["dep" + i]: "^" + i + ".0.0" }, shared, more: [i, "s" + i] });
    }
    const ab = new ArrayBuffer(12);
    new Uint8Array(ab).forEach((_, i, a) => (a[i] = i * 3));
    const map = new Map<any, any>();
    for (let i = 0; i < 6; i++) map.set("key" + i, { value: "v" + i, shared });
    const root: any = {
        items,
        map,
        set: new Set(["x" + n, shared, "y"]),
        views: [new Uint8Array(ab, 1, 4), new Uint16Array(ab, 4, 2), new Uint8Array(ab)],
        err: new RangeError("unread stack " + n),
        lazy: JSON.parse('[{"a":"one"},{"b":["two","three"]},"four"]'),
        when: new Date(86400000 * n),
    };
    root.self = root;
    return root;
}

function hash(text: string): number {
    let h = 5381;
    for (let i = 0; i < text.length; i++) h = ((h * 33) ^ text.charCodeAt(i)) >>> 0;
    return h;
}

const { port1, port2 } = new MessageChannel();
const sameThread: any = await new Promise((resolve) => {
    port2.once("message", resolve);
    port1.postMessage(graph(12));
});
port1.close();
port2.close();
const text = describe(sameThread);
console.log("channel", text.length, hash(text), text.slice(0, 120));
console.log("channel shared", sameThread.self === sameThread, sameThread.set.has(sameThread.items[3].shared));

const w = new Worker(new URL("./_helpers/worker_clone_echo.ts", import.meta.url), { workerData: graph(4) });
const replies: any[] = [];
let wake: (() => void) | null = null;
w.on("message", (m: any) => {
    replies.push(m);
    if (wake) wake();
});
const next = (): Promise<any> =>
    new Promise((resolve) => {
        if (replies.length) return resolve(replies.shift());
        wake = () => {
            wake = null;
            resolve(replies.shift());
        };
    });
const hello = await next();
console.log("workerData", hello.seen.length, hash(hello.seen));
for (const n of [6, 16]) {
    w.postMessage(graph(n));
    const reply = await next();
    const back = describe(reply.back);
    console.log("worker", n, reply.seen.length, hash(reply.seen), reply.seen === back, back.slice(-120));
}
await w.terminate();
clearTimeout(guard);
console.log("done");
