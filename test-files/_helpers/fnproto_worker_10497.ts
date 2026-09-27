// Worker half of test_gap_10497_dispatch_function_prototype_mutation.ts.
// A worker is its own realm: the main thread's Function.prototype mutations
// must NOT be visible here, and this realm's own mutations must dispatch.
import { parentPort } from 'node:worker_threads';

const out: string[] = [];
function f() { return 1; }
const FP = Function.prototype;
out.push('worker proto identity ' + (Object.getPrototypeOf(f) === FP));
out.push('worker sees main helper ' + typeof (f as any).mainHelper10497);
out.push('worker hasOwn call ' + Object.hasOwn(FP, 'call'));
out.push('worker hasOwn hasOwnProperty ' + Object.hasOwn(FP, 'hasOwnProperty'));
(FP as any).workerHelper10497 = function (this: any) { return 'worker-helper:' + this.name; };
let acc = '';
for (let i = 0; i < 300; i++) {
    const r = (f as any).workerHelper10497();
    if (i === 0 || i === 299) acc += r + ';';
}
out.push('worker helper ' + acc);
const o: any = { get(k: string) { return 'own-get:' + k; }, has(k: string) { return k === 'x'; } };
let hits = 0;
for (let i = 0; i < 300; i++) { if (o.has(i % 2 ? 'x' : 'y')) hits++; }
out.push('worker literal ' + o.get('k') + ' ' + hits);
delete (FP as any).workerHelper10497;
out.push('worker helper after delete ' + typeof (f as any).workerHelper10497);
parentPort!.postMessage(out.join('\n'));
