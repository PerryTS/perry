// Native constructor bags prove inherited call/apply/bind only while every
// fixed constructor ancestor still has its sealed definition.
import * as http from 'node:http';
import * as util from 'node:util';
import { EventEmitter } from 'node:events';
import { Stream, Readable, Writable, Duplex, Transform, PassThrough } from 'node:stream';

const request: any = { method: 'GET', httpVersionMajor: 1, httpVersionMinor: 1, headers: {} };
function Response(this: any, req: any) { (http.ServerResponse as any).call(this, req); }
util.inherits(Response, http.ServerResponse);
const response = new (Response as any)(request);
console.log('response', response.statusCode, response instanceof http.ServerResponse);
function Source(this: any) { (Readable as any).call(this, { objectMode: true }); }
util.inherits(Source, Readable);
const source = new (Source as any)();
console.log('source', source.readableObjectMode, source instanceof Readable);
const plain: any = {};
const returned = (Readable as any).call(plain, { objectMode: true });
console.log('plain', returned instanceof Readable, returned === plain, Object.keys(plain).join(','));

// A side effect between the split lookup and call must cause rechecking.
const readableParent = Object.getPrototypeOf(Readable);
const argumentReceiver: any = Object.create(Readable.prototype);
function changeDuringArguments() {
  Object.setPrototypeOf(Readable, { call() { return 'arguments changed'; } });
  return argumentReceiver;
}
console.log('argument relink', (Readable as any).call(changeDuringArguments(), { objectMode: true }), !!argumentReceiver._readableState);
Object.setPrototypeOf(Readable, readableParent);

// An existing own property overwritten: even an unrelated static withdraws
// the complete definition. Restore its observable value afterwards.
const originalFrom = (Readable as any).from;
const replacementFrom = function () { return 'overwritten'; };
(Readable as any).from = replacementFrom;
console.log('own static', (Readable as any).from === replacementFrom, new (Source as any)().readableObjectMode);
(Readable as any).from = originalFrom;

// Direct own call, including replacement and deletion (the underlying
// constructor remains callable throughout).
(http.ServerResponse as any).call = function (_recv: any, arg: any) { return 'call:' + arg; };
console.log('own call', (http.ServerResponse as any).call({}, 'a'));
(http.ServerResponse as any).call = function () { return 'replacement'; };
console.log('replace call', (http.ServerResponse as any).call({}));
delete (http.ServerResponse as any).call;
console.log('delete call', new (Response as any)(request).statusCode);

// Data and accessor descriptors, then delete back to inherited call.
Object.defineProperty(Writable, 'call', { value: function () { return 'defined'; }, configurable: true });
console.log('define data', (Writable as any).call({}));
delete (Writable as any).call;
let reads = 0;
Object.defineProperty(Duplex, 'call', {
  get() { reads++; return function () { return 'accessor'; }; }, configurable: true,
});
console.log('define accessor', (Duplex as any).call({}), reads);
delete (Duplex as any).call;

// [[Prototype]] changes through both public forms must be observed.
const transformParent = Object.getPrototypeOf(Transform);
Object.setPrototypeOf(Transform, { call() { return 'relinked'; } });
console.log('setPrototypeOf', (Transform as any).call({}));
Object.setPrototypeOf(Transform, transformParent);
const passParent = Object.getPrototypeOf(PassThrough);
(PassThrough as any).__proto__ = { call() { return 'dunder'; } };
console.log('__proto__', (PassThrough as any).call({}));
Object.setPrototypeOf(PassThrough, passParent);

// The receiver itself can remain pristine while a fixed ancestor changes.
(Stream as any).call = function () { return 'ancestor'; };
console.log('ancestor call', (http.IncomingMessage as any).call({}), (Readable as any).call({}));
delete (Stream as any).call;
const eventParent = Object.getPrototypeOf(EventEmitter);
Object.setPrototypeOf(EventEmitter, { apply() { return 'ancestor relink'; } });
console.log('ancestor prototype', (http.IncomingMessage as any).apply({}, []));
Object.setPrototypeOf(EventEmitter, eventParent);
