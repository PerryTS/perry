// #10497 / #10502 regression guard: the universal method dispatcher's
// own-override check asks "is this receiver %Function.prototype%?" on every
// dispatched call. That identity is now memoized per realm instead of being
// re-resolved through `globalThis.Function` by name. Everything here must keep
// dispatching exactly as Node does while Function.prototype, individual
// functions and plain objects are mutated underneath hot call sites -- and a
// worker (a separate realm) must see only its own Function.prototype.
import { Worker } from 'node:worker_threads';

const FP = Function.prototype;
function named() { return 'named-body'; }
class K { m() { return 'K.m'; } }
const arrow = () => 'arrow-body';

// 1. The intrinsic itself.
console.log('typeof FP', typeof FP);
console.log('FP() returns', String((FP as any)()));
console.log('tag', Object.prototype.toString.call(FP));
console.log('proto of fn', Object.getPrototypeOf(named) === FP);
console.log('proto of class', Object.getPrototypeOf(K) === FP);
console.log('proto of arrow', Object.getPrototypeOf(arrow) === FP);
try { new (FP as any)(); console.log('new FP: no throw'); }
catch (e) { console.log('new FP throws', (e as Error).constructor.name); }

// 2. Own keys of Function.prototype: inherited Object.prototype methods are
// not own until overwritten; call/apply/bind are.
console.log('hasOwn call', Object.hasOwn(FP, 'call'));
console.log('hasOwn bind', Object.hasOwn(FP, 'bind'));
console.log('hasOwn hasOwnProperty', Object.hasOwn(FP, 'hasOwnProperty'));
console.log('hasOwn valueOf', Object.hasOwn(FP, 'valueOf'));
console.log('FP.hasOwnProperty(apply)', FP.hasOwnProperty('apply'));

// 3. Add a user method to Function.prototype and call it hot through every
// kind of function receiver.
(FP as any).mainHelper10497 = function (this: any) { return 'helper:' + (this.name || '(anon)'); };
let helperLog = '';
for (let i = 0; i < 500; i++) {
    const a = (named as any).mainHelper10497();
    const b = (K as any).mainHelper10497();
    const c = (arrow as any).mainHelper10497();
    if (i === 0 || i === 499) helperLog += a + ',' + b + ',' + c + ';';
}
console.log('added helper', helperLog);
console.log('hasOwn added', Object.hasOwn(FP, 'mainHelper10497'));

// 4. Replace the helper mid-loop; the next call must see the new one.
let swapLog = '';
for (let i = 0; i < 400; i++) {
    if (i === 200) (FP as any).mainHelper10497 = function () { return 'replaced'; };
    const r = (named as any).mainHelper10497();
    if (i === 199 || i === 200 || i === 399) swapLog += i + ':' + r + ';';
}
console.log('replaced helper', swapLog);

// 5. Redefine an inherited Object.prototype method as an OWN property of
// Function.prototype -- it becomes own and wins for every function.
Object.defineProperty(FP, 'valueOf', {
    value: function () { return 'fp-valueOf'; }, writable: true, configurable: true,
});
console.log('hasOwn valueOf after define', Object.hasOwn(FP, 'valueOf'));
console.log('fn.valueOf()', (named as any).valueOf());
console.log('obj.valueOf unaffected', typeof ({} as any).valueOf());

// 6. An own property on ONE function beats Function.prototype.
const g: any = function g() { return 'g-body'; };
g.mainHelper10497 = () => 'own-on-g';
g.toString = () => 'own-toString';
console.log('own beats proto', g.mainHelper10497(), (named as any).mainHelper10497());
console.log('own toString', String(g), g.toString());
g.call = () => 'own-call';
console.log('own call', g.call(null), named.call(null));

// 7. Object.setPrototypeOf on a function swaps its whole method set.
const h: any = function h() { return 'h-body'; };
const altProto = { greet() { return 'alt-greet'; }, mainHelper10497() { return 'alt-helper'; } };
Object.setPrototypeOf(h, altProto);
console.log('setPrototypeOf', h.greet(), h.mainHelper10497(), typeof h.call, typeof h.bind);
console.log('h still callable', h());
Object.setPrototypeOf(h, FP);
console.log('restored', typeof h.greet, h.mainHelper10497(), h.call(null));

// 8. Replace call/bind on Function.prototype, then restore. Results are
// captured and printed only after the restore: the host's own console
// plumbing may itself go through Function.prototype.call.
const origCall = FP.call;
const origBind = FP.bind;
(FP as any).call = function (this: any) { return 'patched-call:' + this.name; };
(FP as any).bind = function (this: any) { return 'patched-bind:' + this.name; };
const patchedCall = (named as any).call(null);
const patchedBind = (named as any).bind(null);
(FP as any).call = origCall;
(FP as any).bind = origBind;
console.log('patched', patchedCall, patchedBind);
console.log('unpatched', named.call(null), named.bind(null)());

// 9. Plain objects: object-literal methods named like builtins (the qs
// `side-channel` shape) dispatch to the literal's own methods, and an own
// method added mid-loop to a prototype-method receiver wins from then on.
const channel: any = {
    store: new Map<any, any>(),
    get(k: any) { return this.store.get(k); },
    set(k: any, v: any) { this.store.set(k, v); },
    has(k: any) { return this.store.has(k); },
};
let chanSum = 0;
for (let i = 0; i < 1000; i++) {
    channel.set(i % 10, i);
    if (channel.has(i % 7)) chanSum += channel.get(i % 7);
}
console.log('literal channel', chanSum);

class Buf { data = 'x'; getBytes() { return this.data; } }
const buf: any = new Buf();
let bufLog = '';
for (let i = 0; i < 400; i++) {
    if (i === 250) buf.getBytes = () => 'own-bytes';
    const r = buf.getBytes();
    if (i === 249 || i === 250 || i === 399) bufLog += i + ':' + r + ';';
}
console.log('own added mid-loop', bufLog);

// 10. Own overrides on builtin-kind receivers.
const m: any = new Map([[1, 'one']]);
const before = m.get(1);
m.get = (k: any) => 'own-map-get:' + k;
console.log('map own get', before, m.get(1));
const d: any = new Date(0);
d.getTime = () => 42;
console.log('date own getTime', d.getTime());

// 11. Reassigning the global binding does not change the intrinsic.
const RealFunction = globalThis.Function;
(globalThis as any).Function = function FakeFunction() {};
console.log('after global reassign', Object.getPrototypeOf(named) === FP,
    (named as any).mainHelper10497(), Object.hasOwn(FP, 'hasOwnProperty'));
(globalThis as any).Function = RealFunction;
console.log('global restored', globalThis.Function === RealFunction);

// 12. Delete the helper; dispatch must see it gone.
delete (FP as any).mainHelper10497;
console.log('after delete', typeof (named as any).mainHelper10497, typeof g.mainHelper10497);

// 13. A worker is its own realm.
const worker = new Worker(new URL('./_helpers/fnproto_worker_10497.ts', import.meta.url));
worker.on('message', (msg: string) => {
    console.log(msg);
    worker.terminate().then(() => process.exit(0));
});
setTimeout(() => process.exit(2), 10000);
