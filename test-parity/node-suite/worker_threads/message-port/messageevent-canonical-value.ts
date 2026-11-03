import { MessageChannel } from 'node:worker_threads';
const channel = new MessageChannel();
const ports = { *[Symbol.iterator]() { yield channel.port1; yield channel.port2; } };
const event = new MessageEvent('x', { data: 42, origin: 'origin', lastEventId: 'id', source: channel.port1, ports });
console.log(event instanceof MessageEvent, event instanceof Event, Object.getPrototypeOf(event) === MessageEvent.prototype);
console.log(event.data, event.origin, event.lastEventId, event.source === channel.port1);
console.log(event.ports.length, event.ports[0] === channel.port1, event.ports[1] === channel.port2);
console.log('frozen', Object.isFrozen(event.ports));
const OriginalEvent = MessageEvent;
const OriginalPort = MessagePort;
const stable = new MessageEvent('stable', { get data() {
  globalThis.MessageEvent = function Replacement() {} as any;
  globalThis.MessagePort = function ReplacementPort() {} as any;
  return 3;
}, source: channel.port1 });
console.log('intrinsic', Object.getPrototypeOf(stable) === OriginalEvent.prototype, stable.constructor === OriginalEvent, stable.initMessageEvent('fresh') instanceof OriginalEvent);
globalThis.MessageEvent = OriginalEvent;
globalThis.MessagePort = OriginalPort;
channel.port2.onmessage = (message) => {
  console.log('delivery', message instanceof MessageEvent, message instanceof Event, message.constructor === MessageEvent);
  console.log(message.data, message.target === channel.port2, message.currentTarget === channel.port2, message.eventPhase, message.isTrusted);
  const replacement = message.initMessageEvent('fresh');
  console.log(message.type, replacement.type, replacement !== message);
  setImmediate(() => {
    console.log('cleanup', message.currentTarget === null, message.eventPhase, message.target === channel.port2);
    channel.port1.close(); channel.port2.close();
  });
};
channel.port1.postMessage(7);
const prototypePort = Object.create(OriginalPort.prototype);
Object.defineProperty(OriginalPort, Symbol.hasInstance, { value: () => false, configurable: true });
const references = new MessageEvent('references', { source: prototypePort, ports: [prototypePort, channel.port1] });
console.log('references', references.source === prototypePort, references.ports[0] === prototypePort, references.ports[1] === channel.port1);
delete OriginalPort[Symbol.hasInstance];
