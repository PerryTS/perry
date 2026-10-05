// Worker postMessage uses Node's structured clone in both directions: every
// cloneable kind arrives with its value, shared references and cycles stay
// shared, a Buffer arrives as a Uint8Array, views keep their offset into the
// cloned buffer, the transfer list detaches the sender's ArrayBuffers, and a
// value that cannot be cloned throws DataCloneError instead of arriving as
// undefined.
import { Worker } from "node:worker_threads";
import { describe } from "./_helpers/clone_describe.ts";

const guard = setTimeout(() => {
    console.log("timeout");
    process.exit(2);
}, 20000);

const wdab = new ArrayBuffer(4);
new Uint8Array(wdab).set([1, 2, 3, 4]);
const w = new Worker(new URL("./_helpers/worker_clone_echo.ts", import.meta.url), {
    workerData: { ab: wdab, map: new Map([["k", 1]]), list: [1, 2] },
    transferList: [wdab],
});
console.log("workerData buffer after ctor", wdab.byteLength);

const replies: any[] = [];
let wake: (() => void) | null = null;
w.on("message", (m: any) => {
    replies.push(m);
    if (wake) wake();
});
function next(): Promise<any> {
    return new Promise((resolve) => {
        if (replies.length) return resolve(replies.shift());
        wake = () => {
            wake = null;
            resolve(replies.shift());
        };
    });
}

async function send(label: string, value: any, transfer?: any[]) {
    try {
        if (transfer) w.postMessage(value, transfer);
        else w.postMessage(value);
    } catch (e: any) {
        console.log(label, "throws", e.name);
        return;
    }
    const reply = await next();
    console.log(label, "| worker saw", reply.seen);
    console.log(label, "| came back", describe(reply.back));
}

console.log(describe(await next()));

const q: any = { name: "nitro", spec: { range: "^2.0.0" }, a: 1, b: 2, c: 3, d: 4, e: 5, f: 6, g: 7, h: 8 };
await send("spread", { ...q, id: 7 });
await send("assign", Object.assign({}, q, { id: 9, extra: [1] }));
await send("primitives", [1.5, -0, NaN, Infinity, "héllo ✓", true, null, undefined, 12n, -(2n ** 70n)]);
await send("holes", [1, , 3]);
const sparse: any[] = [];
sparse[4] = "x";
await send("sparse", sparse);
const shared = { s: 1 };
await send("shared", { a: shared, b: shared, list: [shared] });
const cyc: any = { name: "cyc" };
cyc.self = cyc;
cyc.arr = [cyc];
await send("cycle", cyc);
await send("date-regexp", [new Date(1700000000000), /a+b/gi]);
const readonlyRegExp: any = /b+/gy;
readonlyRegExp.lastIndex = 7;
readonlyRegExp.note = "sender only";
Object.defineProperty(readonlyRegExp, "lastIndex", { writable: false });
await send("regexp-readonly", readonlyRegExp);
class RegExpChild extends RegExp {
    #stamp = 1;
    stamp() { return this.#stamp; }
}
const childRegExp: any = new RegExpChild("c+", "gi");
childRegExp.lastIndex = 9;
childRegExp.note = "sender only";
await send("regexp-subclass", childRegExp);
const key = { k: 1 };
await send("map-set", new Map<any, any>([[key, "v"], ["s", new Set([1, "a", key])]]));
const err: any = new TypeError("bad", { cause: { why: 1 } });
err.code = "E_BAD";
await send("errors", [err, new RangeError("r"), new Error("plain")]);
class P {
    a = 1;
    b = "two";
    m() {
        return 3;
    }
}
await send("class", new P());
const hidden: any = { shown: 1 };
Object.defineProperty(hidden, "hidden", { value: 2, enumerable: false });
hidden[Symbol("s")] = 3;
await send("hidden", hidden);
await send("lazy-json", JSON.parse('[1,{"a":[2,3]},"x",[]]'));

const ab = new ArrayBuffer(8);
new Uint8Array(ab).set([1, 2, 3, 4, 5, 6, 7, 8]);
await send("arraybuffer", ab);
await send("arraybuffer-list", [ab, ab, new ArrayBuffer(2)]);
const buf = Buffer.alloc(6);
buf.write("buffer");
await send("buffer", buf);
await send("buffer-subarray", buf.subarray(2, 5));
await send("typed", [new Uint32Array([1, 2, 0xffffffff]), new Float64Array([0.5, -1]), new Int16Array([-2, 3]), new BigInt64Array([5n, -6n])]);
const ab16 = new ArrayBuffer(16);
new Uint8Array(ab16).forEach((_, i, a) => (a[i] = i));
await send("views-one-buffer", [new Uint8Array(ab16, 4, 8), new Uint16Array(ab16, 2, 3), new DataView(ab16, 1, 2)]);

const tab = new ArrayBuffer(4);
const tview = new Uint8Array(tab);
tview.set([4, 3, 2, 1]);
await send("transfer", { tab, tview }, [tab]);
console.log("sender after transfer", tab.byteLength, tview.length, (tab as any).detached);
const own = new Uint8Array([7, 7, 7]);
await send("transfer-u8-buffer", own, [own.buffer]);
console.log("sender u8 after transfer", own.length, own.byteLength);

await send("function", { f: () => 1 });
await send("symbol", Symbol("s"));
await send("promise", Promise.resolve(1));
const dup = new ArrayBuffer(1);
await send("duplicate-transfer", dup, [dup, dup]);
await send("bad-transfer", 1, [{}]);
await send("detached-transfer", 1, [tab]);
console.log("buffer still attached after failed post", dup.byteLength);

w.postMessage({ cmd: "post-fn" });
console.log((await next()).seen);
w.postMessage({ cmd: "send-ab" });
const t = await next();
console.log(t.seen, describe(t.back));
console.log((await next()).seen);

await w.terminate();
clearTimeout(guard);
console.log("done");
