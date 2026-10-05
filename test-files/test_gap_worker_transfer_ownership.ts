import { Worker, MessageChannel } from "node:worker_threads";
declare function gc(): void;
const guard = setTimeout(() => process.exit(2), 60000);
const wd = new ArrayBuffer(16);
new Uint8Array(wd)[0] = 41;
const w = new Worker(new URL("./_helpers/worker_transfer_ownership.ts", import.meta.url), { workerData: wd, transferList: [wd] });
console.log("workerData detached", wd.byteLength);
const replies: any[] = [];
let wake: any = null;
w.on("message", (m: any) => { replies.push(m); if (wake) wake(); });
function next(): Promise<any> {
    return new Promise(resolve => {
        if (replies.length) return resolve(replies.shift());
        wake = () => { wake = null; resolve(replies.shift()); };
    });
}
console.log("workerData", await next());
for (let round = 0; round < 4; round++) {
    const ab = new ArrayBuffer(8 * 1024 * 1024);
    const u8 = new Uint8Array(ab);
    u8[0] = round + 1;
    u8[u8.length - 1] = 199;
    const offset = new Uint8Array(ab, 8, 16);
    const u16 = new Uint16Array(ab, 8, 4);
    const dv = new DataView(ab, 8, 16);
    const shared = { tag: "shared" + round };
    const payload: any = { ab, u8, offset, u16, dv, again: ab, shared, aliases: [shared, u8], lazy: JSON.parse('[{"x":[1,2]},"three"]'), err: new Error("build stack") };
    payload.self = payload;
    // The lazy value and error stack require writer retries. None of the
    // transfer sources may be taken during an unsuccessful/retried walk.
    try { w.postMessage([payload, () => 1], [ab]); } catch (e: any) { console.log("failure", e.name, ab.byteLength, u8[0]); }
    w.postMessage(payload, [ab]);
    console.log("sender", ab.byteLength, u8.length, offset.length, u16.length);
    const m = await next();
    console.log("receiver", m.ab.byteLength, m.u8[0], m.u8[m.u8.length - 1], m.offset[0], m.u16[0], m.dv.getUint8(0));
    console.log("aliases", m.ab === m.again, m.u8.buffer === m.ab, m.offset.buffer === m.ab, m.u16.buffer === m.ab, m.dv.buffer === m.ab, m.self === m, m.shared === m.aliases[0], m.u8 === m.aliases[1]);
    m.dv.setUint8(1, 66);
    console.log("write", m.offset[1], m.u8[9]);
    for (let i = 0; i < 30; i++) {
        const churn = { text: "churn-" + i, nested: [i, { round }] };
        const dead = Buffer.alloc(1024 * 1024);
        dead[0] = i;
        if (churn.nested.length !== 2) process.exit(3);
    }
    if (typeof gc === "function") gc();
    if (m.u8[0] !== round + 1 || m.u8[9] !== 66 || m.u8[m.u8.length - 1] !== 199) process.exit(7);
}
const own = new Uint8Array([3, 4, 5, 6]);
w.postMessage({ own }, [own.buffer]);
const fallback = await next();
console.log("inline fallback", own.length, fallback.own.length, fallback.own[2]);
const { port1, port2 } = new MessageChannel();
port1.postMessage(new ArrayBuffer(8 * 1024 * 1024), []);
const discarded = new ArrayBuffer(8 * 1024 * 1024);
port1.postMessage(discarded, [discarded]);
port1.close(); port2.close();
console.log("discarded detached", discarded.byteLength);
await w.terminate(); clearTimeout(guard);
console.log("done");
