import { Readable, Writable, Duplex, Transform, PassThrough } from 'node:stream';
import { EventEmitter } from 'node:events';
console.log('constructor fixture', true);
// Each ordinary constructor assignment must respect a non-writable inherited key.
for (const C of [Readable, Writable, Duplex, Transform, PassThrough]) {
  const old = Object.getOwnPropertyDescriptor(C.prototype, '_events');
  Object.defineProperty(C.prototype, '_events', { value: undefined, writable: false, configurable: true });
  try { new C(); console.log(C.name, 'did not throw'); }
  catch (e) { console.log(C.name, e instanceof TypeError); }
  if (old) Object.defineProperty(C.prototype, '_events', old);
  else delete C.prototype._events;
}
const old = Object.getOwnPropertyDescriptor(EventEmitter.prototype, '_eventsCount');
Object.defineProperty(EventEmitter.prototype, '_eventsCount', { value: 0, writable: false, configurable: true });
try { new EventEmitter(); console.log('emitter did not throw'); }
catch (e) { console.log('emitter', e instanceof TypeError); }
if (old) Object.defineProperty(EventEmitter.prototype, '_eventsCount', old);
else delete EventEmitter.prototype._eventsCount;
