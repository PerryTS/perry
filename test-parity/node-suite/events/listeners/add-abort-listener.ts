import { addAbortListener } from "node:events";

const ac = new AbortController();
let count = 0;
const disposable: any = addAbortListener(ac.signal, () => { count++; });
console.log("dispose type:", typeof disposable?.[Symbol.dispose]);
ac.abort();
ac.abort();
console.log("count:", count);
const ac2 = new AbortController();
const d2: any = addAbortListener(ac2.signal, () => { count += 10; });
d2?.[Symbol.dispose]?.();
ac2.abort();
console.log("count after dispose:", count);
