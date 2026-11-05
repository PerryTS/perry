import { Readable, Duplex } from 'node:stream';
import { EventEmitter } from 'node:events';
console.log('constructor fixture', true);
let seen;
Object.defineProperty(Readable.prototype, '_readableState', {
  configurable: true,
  set(state) {
    seen = this;
    console.log('readable setter', Object.keys(this).join(','), Object.hasOwn(this, '_readableState'));
    Object.defineProperty(this, '_readableState', { value: state, writable: true, enumerable: true, configurable: true });
  }
});
const r = new Readable();
console.log('readable after', seen === r, Object.keys(r).join(','));
delete Readable.prototype._readableState;
Object.defineProperty(Duplex.prototype, '_readableState', {
  configurable: true,
  set(state) {
    console.log('duplex setter', Object.keys(this).join(','), Object.hasOwn(this, '_writableState'));
    Object.defineProperty(this, '_writableState', { value: 17, writable: false, configurable: true });
    Object.defineProperty(this, '_readableState', { value: state, writable: true, enumerable: true, configurable: true });
  }
});
try { new Duplex(); console.log('duplex did not throw'); }
catch (e) { console.log('duplex readonly', e instanceof TypeError); }
delete Duplex.prototype._readableState;
const prior = Object.getOwnPropertyDescriptor(EventEmitter.prototype, '_maxListeners');
Object.defineProperty(EventEmitter.prototype, '_maxListeners', {
  configurable: true, get() { return undefined; },
  set(value) { console.log('emitter setter', Object.keys(this).join(','), value === undefined); }
});
const e = new EventEmitter();
console.log('emitter after', Object.keys(e).join(','), Object.hasOwn(e, '_maxListeners'));
if (prior) Object.defineProperty(EventEmitter.prototype, '_maxListeners', prior);
else delete EventEmitter.prototype._maxListeners;
