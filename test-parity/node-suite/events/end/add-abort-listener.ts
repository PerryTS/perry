import { addAbortListener } from "node:events";

const ctrl = new AbortController();
let fired = 0;
const disposable = addAbortListener(ctrl.signal, () => { fired++; });
ctrl.abort();
console.log("fired:", fired);
console.log("dispose type:", typeof (disposable as any)[Symbol.dispose]);
