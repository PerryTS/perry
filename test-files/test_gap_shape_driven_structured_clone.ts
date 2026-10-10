// parity-env: PERRY_JSON_TAPE=1
import { Worker } from 'node:worker_threads';
import { describe } from './_helpers/clone_describe.ts';
function make(): any {
  class Record { id = 7; kind = 3; name = 'shared text longer than a short string'; #hidden = 19; }
  const shared: any = { value: 4 };
  const frozenArray = Object.freeze([Object.freeze({value: 7}), undefined, 9]);
  const root: any = { frozenArray, records: [new Record(), Object.freeze({id: 8, kind: 1, name: 'shared text longer than a short string'})], shared, alias: shared };
  root.self = root;
  root.order = { zebra: 1, '20': 20, '3': 3, alpha: 2, '03': 30 };
  root.hidden = { shown: 3 };
  Object.defineProperty(root.hidden, 'secret', { value: 11, enumerable: false });
  root.hidden[Symbol('hidden')] = 12;
  Object.defineProperty(root.hidden, Symbol('ignored-getter'), {enumerable: true, get() {throw new Error('symbol getter must not run');}});
  root.getter = { first: 1 };
  Object.defineProperty(root.getter, 'computed', { enumerable: true, get() { return 42; } });
  const mutator: any = {};
  Object.defineProperty(mutator, 'a', { enumerable: true, get() { delete mutator.b; mutator.c = 9; return 5; } });
  mutator.b = 2; root.mutator = mutator;
  const parent: any = {};
  const child: any = {};
  Object.defineProperty(child, 'value', { enumerable: true, get() { delete parent.tail; parent.extra = 13; return 6; } });
  parent.child = child; parent.tail = 10; root.nestedMutation = parent;
  const holes: any[] = [1, , undefined, -0, NaN]; holes[10000] = 7; root.holes = holes;
  const props: any = [5, , 8];
  Object.defineProperty(props, '1', {enumerable: true, get() {return 6;}});
  Object.defineProperty(props, '2', {enumerable: false, value: 8});
  Object.defineProperty(props, 'extra', {enumerable: true, get() {return 'array-own';}});
  root.arrayProps = props;
  const visibility: any = {};
  Object.defineProperty(visibility, 'first', {enumerable: true, get() {Object.defineProperty(visibility, 'hidden', {enumerable: true, value: 9}); return 1;}});
  Object.defineProperty(visibility, 'hidden', {enumerable: false, configurable: true, value: 2});
  root.visibility = visibility;
  root.numeric = [-0, NaN, Infinity, -Infinity, 3.25];
  const arrayMutation: any[] = [{}, 2];
  const arrayChild: any = {};
  Object.defineProperty(arrayChild, 'value', {enumerable: true, get() {
    Object.defineProperty(arrayMutation, '1', {enumerable: true, get() {return 9;}});
    arrayMutation.extra = 11; return 4;
  }});
  arrayMutation[0] = arrayChild; root.arrayMutation = arrayMutation;
  const setterOnly: any = {};
  Object.defineProperty(setterOnly, 'value', {enumerable: true, set(value: any) {throw new Error('setter must not run');}});
  root.setterOnly = setterOnly;
  root.primitives = [-0, NaN, Infinity, -Infinity, undefined, null, false, true, 1.25, 123n, -(2n ** 70n), 'héllo ✓'];
  const ab = new ArrayBuffer(16); const view = new Uint8Array(ab); view.set([3, 4, 5, 6]);
  root.views = [ab, new Uint8Array(ab, 1, 3), new Uint16Array(ab, 2, 2), new DataView(ab, 1, 3), new Float64Array([0.5, -1]), new BigInt64Array([5n, -6n]), new Int8Array([-2, 3]), new Uint8ClampedArray([256, -1]), new Int16Array([-9, 11]), new Uint32Array([4294967295]), new Int32Array([-3]), new Float32Array([0.25]), new BigUint64Array([7n])];
  root.when = new Date(1700000000000); root.regex = /a+b/gi; root.regex.lastIndex = 7;
  root.map = new Map<any, any>([[shared, root], ['second', shared]]); root.set = new Set<any>([shared, root, 3]);
  return root;
}
function summarize(value: any): string {
  const holes = value.holes; delete value.holes;
  const text = describe(value); value.holes = holes;
  return text + '|holes=' + value.holes.length + ':' + (1 in value.holes) + ':' + (2 in value.holes) + ':' + (9000 in value.holes) + ':' + value.holes[10000]
    + '|array-props=' + Object.keys(value.arrayProps).join(',') + ':' + value.arrayProps.extra + ':' + (2 in value.arrayProps)
    + '|keys=' + Object.keys(value.order).join(',') + '|plain=' + (Object.getPrototypeOf(value.records[0]) === Object.prototype)
    + '|mutable=' + !Object.isFrozen(value.records[1]) + '|identity=' + (value.alias === value.shared) + ':' + (value.self === value) + ':' + (value.views[1].buffer === value.views[0]);
}
const direct = structuredClone(make());
console.log('direct ' + summarize(direct));
direct.records[1].id = 99; console.log('mutable ' + direct.records[1].id);
const worker = new Worker(new URL('./_helpers/worker_shape_copy_echo.ts', import.meta.url));
const workerThrow: any = {}; Object.defineProperty(workerThrow, 'value', {enumerable: true, get() {throw new RangeError('worker-getter-threw');}});
try { worker.postMessage(workerThrow); } catch (e: any) {console.log('worker-throws ' + e.name + ':' + e.message);}
try { worker.postMessage({bad: () => 1}); } catch (e: any) {console.log('worker-function ' + e.name);}
const response: any = await new Promise((resolve, reject) => {
  worker.on('message', resolve); worker.on('error', reject); worker.postMessage(make());
});
console.log('worker ' + summarize(response));
await worker.terminate();
let count = 0;
const once: any = {}; Object.defineProperty(once, 'value', {enumerable: true, get() { count++; return JSON.parse('[{"id":1},{"id":2},{"id":3}]'); }});
const oneClone: any = structuredClone(once); console.log('getter-once ' + count + ':' + oneClone.value[0].id);
const throwing: any = {}; Object.defineProperty(throwing, 'value', {enumerable: true, get() {throw new RangeError('getter-threw');}});
try { structuredClone(throwing); } catch (e: any) { console.log('throws ' + e.name + ':' + e.message); }
console.log('after-throw ' + structuredClone({id: 19}).id);

try { structuredClone({bad: () => 1}); } catch (e: any) { console.log('function ' + e.name); }
try { structuredClone(Math.abs); } catch (e: any) { console.log('native-function ' + e.name); }
try { structuredClone(Symbol('no')); } catch (e: any) { console.log('symbol ' + e.name); }
const transfer = new ArrayBuffer(4); new Uint8Array(transfer)[0] = 17;
const transferred: any = structuredClone({transfer, view: new Uint8Array(transfer)}, {transfer: [transfer]});
console.log('transfer ' + transfer.byteLength + ':' + transferred.transfer.byteLength + ':' + transferred.view[0] + ':' + (transferred.view.buffer === transferred.transfer));
const kept = new ArrayBuffer(4);
try { structuredClone({kept, bad: () => 1}, {transfer: [kept]}); } catch (e: any) {console.log('failed-transfer ' + e.name + ':' + kept.byteLength);}
