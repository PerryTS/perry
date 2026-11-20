// Constructor conversions precede new.target.prototype and run once.
const order: string[] = [];
const options: any = {};
for (const name of ['bubbles', 'cancelable', 'composed', 'data', 'lastEventId', 'origin', 'ports', 'source']) {
  Object.defineProperty(options, name, { get() { order.push(name); return undefined; } });
}
const target = new Proxy(function () {}, {
  get(object, name) {
    if (name === 'prototype') order.push('prototype');
    return Reflect.get(object, name);
  }
});
Reflect.construct(MessageEvent, [{ toString() { order.push('type'); return 'x'; } }, options], target);
console.log(order.join(','));
const original = new MessageEvent('old', { data: 1 });
const fresh = original.initMessageEvent('new', true, true, 2, 'origin', 'id', null, []);
console.log(original.type, original.data, fresh.type, fresh.data, fresh !== original);
for (const action of [
  () => new MessageEvent(),
  () => original.initMessageEvent(),
  () => MessageEvent.prototype.initMessageEvent.call({}, 'x'),
  () => new MessageEvent('x', { ports: null }),
  () => new MessageEvent('x', { ports: [{}] }),
  () => new MessageEvent('x', { source: {} })
]) {
  try { action(); console.log('unexpected'); }
  catch (error) { console.log(error.name, error.code ?? 'none'); }
}
const sentinel = {};
try { new MessageEvent('x', { get data() { throw sentinel; } }); }
catch (error) { console.log('identity', error === sentinel); }

// Existing delivery queues honor immediate stop without changing raw listeners.
import { MessageChannel, BroadcastChannel } from 'node:worker_threads';
const results: string[] = [];
let completed = 0;
function report(index: number, value: string) {
  results[index] = value;
  if (++completed === 4) for (const result of results) console.log(result);
}
for (const immediate of [false, true]) {
  const channel = new MessageChannel();
  const calls: string[] = [];
  channel.port2.on('message', () => calls.push('raw'));
  channel.port2.addEventListener('message', (event) => {
    calls.push('first');
    if (immediate) event.stopImmediatePropagation(); else event.stopPropagation();
    setImmediate(() => {
      report(immediate ? 2 : 0, `port-stop ${immediate} ${calls.join(',')}`);
      channel.port1.close(); channel.port2.close();
    });
  });
  channel.port2.addEventListener('message', () => calls.push('second'));
  channel.port2.onmessage = () => calls.push('property');
  channel.port1.postMessage(1);
  const sender = new BroadcastChannel(`messageevent-stop-${immediate}`);
  const receiver = new BroadcastChannel(`messageevent-stop-${immediate}`);
  const broadcastCalls: string[] = [];
  receiver.onmessage = (event) => {
    broadcastCalls.push('property');
    if (immediate) event.stopImmediatePropagation(); else event.stopPropagation();
    setImmediate(() => {
      report(immediate ? 3 : 1, `broadcast-stop ${immediate} ${broadcastCalls.join(',')}`);
      sender.close(); receiver.close();
    });
  };
  receiver.addEventListener('message', () => broadcastCalls.push('listener'));
  sender.postMessage(1);
}
