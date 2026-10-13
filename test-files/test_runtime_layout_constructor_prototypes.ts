import { Readable } from 'node:stream';
import { EventEmitter } from 'node:events';
console.log('constructor fixture', true);
const ep = EventEmitter.prototype;
const e1 = new EventEmitter();
const replacement = Object.create(ep);
replacement._maxListeners = 7;
EventEmitter.prototype = replacement;
const e2 = new EventEmitter();
console.log('emitter prototypes', Object.getPrototypeOf(e1) === ep, Object.getPrototypeOf(e2) === replacement);
console.log('emitter inherited max', e2._maxListeners, Object.hasOwn(e2, '_maxListeners'), Object.keys(e2).join(','));
EventEmitter.prototype = ep;
const rp = Readable.prototype;
const r1 = new Readable();
const rreplacement = Object.create(rp);
rreplacement.marker = 23;
Readable.prototype = rreplacement;
const r2 = new Readable();
console.log('readable prototypes', Object.getPrototypeOf(r1) === rp, Object.getPrototypeOf(r2) === rreplacement, r2.marker);
Readable.prototype = rp;
class E extends EventEmitter { field = 41; }
class R extends Readable { field = 43; _read() {} }
const e = new E();
const r = new R();
console.log('subclasses', e.field, r.field, e instanceof E, e instanceof EventEmitter, r instanceof R, r instanceof Readable);
console.log('subclass keys', Object.keys(e).join(','), Object.keys(r).join(','), Object.hasOwn(r, '_read'));
