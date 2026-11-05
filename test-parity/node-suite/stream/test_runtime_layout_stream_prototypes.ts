import { Readable, Writable, Duplex, Transform, PassThrough, Stream } from 'node:stream';
import { EventEmitter, EventEmitterAsyncResource } from 'node:events';
function check(proto: any, names: string[], includeConfigurable: boolean = true) {
  console.log(Object.getOwnPropertyNames(proto).filter((n: string) => names.includes(n)).join(','));
  for (const name of names) {
    const d = Object.getOwnPropertyDescriptor(proto, name)!;
    console.log(name, d.writable, d.enumerable, typeof d.value, typeof d.get, typeof d.set);
    if (includeConfigurable) console.log('configurable', d.configurable);
  }
  proto.laneExtra = 17;
  console.log(Object.getOwnPropertyNames(proto).slice(-1).join(','), proto.laneExtra);
  const extra = Object.getOwnPropertyDescriptor(proto, 'laneExtra')!;
  console.log(extra.writable, extra.enumerable, extra.configurable);
  delete proto.laneExtra;
}
const r = new Readable({read() {}});
const r2 = new Readable({read() {}});
const w = new Writable({write(chunk: any, enc: any, cb: any) { cb(); }});
const w2 = new Writable({write(chunk: any, enc: any, cb: any) { cb(); }});
const rp = Object.getPrototypeOf(r._readableState);
const wp = Object.getPrototypeOf(w._writableState);
console.log(rp === Object.getPrototypeOf(r2._readableState), Object.getPrototypeOf(rp) === Object.prototype);
console.log(wp === Object.getPrototypeOf(w2._writableState), Object.getPrototypeOf(wp) === Object.prototype);
check(rp, ['objectMode','ended','endEmitted','readableListening','resumeScheduled','errorEmitted','emitClose','autoDestroy','destroyed','closed','closeEmitted','dataEmitted','errored','encoding'], false);
check(wp, ['objectMode','finalCalled','needDrain','ending','ended','finished','destroyed','decodeStrings','errorEmitted','emitClose','autoDestroy','closed','closeEmitted','errored'], false);
console.log(Object.getPrototypeOf(Duplex.prototype) === Readable.prototype);
check(Duplex.prototype, ['constructor','write','cork','uncork']);
console.log(Object.getPrototypeOf(Transform.prototype) === Duplex.prototype);
check(Transform.prototype, ['constructor','_transform']);
console.log(Object.getPrototypeOf(PassThrough.prototype) === Transform.prototype);
check(PassThrough.prototype, ['constructor']);
console.log(Object.getPrototypeOf(EventEmitter.prototype) === Object.prototype);
check(EventEmitter.prototype, ['constructor','_events','_eventsCount','_maxListeners','setMaxListeners','getMaxListeners','emit','addListener','on','prependListener','once','prependOnceListener','removeListener','off','removeAllListeners','listeners','rawListeners','listenerCount','eventNames']);
console.log(EventEmitter.prototype.addListener === EventEmitter.prototype.on, EventEmitter.prototype.off === EventEmitter.prototype.removeListener);
console.log(Object.getPrototypeOf(EventEmitterAsyncResource.prototype) === EventEmitter.prototype);
check(EventEmitterAsyncResource.prototype, ['constructor','emit','emitDestroy','asyncId','triggerAsyncId','asyncResource']);
console.log(Object.getPrototypeOf(Stream.prototype) === EventEmitter.prototype);
check(Stream.prototype, ['constructor','eventNames']);
