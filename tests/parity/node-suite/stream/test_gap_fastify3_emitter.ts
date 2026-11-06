import { Readable } from 'node:stream';
import { EventEmitter } from 'node:events';

const output: string[] = [];
for (const emitter of [new EventEmitter(), new Readable({ read() {} })]) {
  Object.defineProperty(emitter, 'unrelated', { value: 7, enumerable: false });
  let calls = 0;
  function listener(...args: any[]) {
    calls++;
    output.push(args.map(v => String(v)).join(':'));
  }
  emitter.once('x', listener);
  const wrapper = emitter.rawListeners('x')[0];
  output.push(String(wrapper.listener === listener));
  emitter.emit('x', 1, 2, 3, 4, 5, 6, 7, 8, 9);
  emitter.emit('x', 10);
  wrapper(11);
  output.push(String(calls), String(emitter.listenerCount('x')));
  const symbol = Symbol('event');
  emitter.once(symbol, listener);
  emitter.emit(symbol, 'symbol');
  emitter.on('keep', listener);
  emitter.on('last', listener);
  emitter.removeListener('last', listener);
  emitter.removeAllListeners();
  output.push(String((emitter as any)._eventsCount));
  const retained = (emitter as any)._events;
  emitter.on('only', listener);
  emitter.removeListener('only', listener);
  output.push(String(retained !== (emitter as any)._events), String(retained.only === listener));
}

const emitter = new Readable({ read() {} });
let events = (emitter as any)._events;
Object.defineProperty(emitter, '_events', {
  configurable: true,
  get() { return events; },
  set(value) { events = value; }
});
function listener(value: any) { output.push(String(value)); }
emitter.once('x', listener);
emitter.emit('x', 'accessor');
output.push(String((emitter as any)._eventsCount));

const meta = new Readable({ read() {} });
meta.on('newListener', (event: any, fn: any) => {
  if (event === 'x') output.push(String(fn === listener));
});
meta.on('removeListener', (event: any, fn: any) => {
  if (event === 'x') output.push(String(fn === listener));
});
meta.prependOnceListener('x', listener);
meta.emit('x', 'meta');
console.log(output.join('|'));
