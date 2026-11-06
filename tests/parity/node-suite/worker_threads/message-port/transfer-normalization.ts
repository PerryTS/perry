import { BroadcastChannel, MessageChannel, Worker, receiveMessageOnPort } from 'node:worker_threads';

const { port1, port2 } = new MessageChannel();
function send(label: string, wrap: (buffer: ArrayBuffer) => any) {
  const buffer = new ArrayBuffer(4);
  port1.postMessage({ buffer }, wrap(buffer));
  console.log(label, buffer.byteLength, receiveMessageOnPort(port2)?.message.buffer.byteLength);
}
send('array', b => [b]);
send('set', b => new Set([b]));
send('generator', b => (function* () { yield b; })());
send('options', b => ({ transfer: new Set([b]) }));

const order: string[] = [];
send('fallback', b => ({
  [Symbol.iterator]() { order.push('iterator'); return { get next() { order.push('next'); return 42; } }; },
  get transfer() { order.push('transfer'); return [b]; },
}));
console.log('order', order.join(','));

for (const nested of [false, true]) {
  try { port1.postMessage({}, nested ? { transfer: 5 } as any : 5 as any); }
  catch (error: any) { console.log('invalid', nested, error.name, error.code, error.message); }
}
try { port1.postMessage({}, { transfer: { [Symbol.iterator]: () => ({ next: null }) } } as any); }
catch (error: any) { console.log('malformed', error.name, error.code, error.message); }

for (const phase of ['next', 'done', 'value', 'after-invalid']) {
  const buffer = new ArrayBuffer(4);
  const expected = new Error(phase);
  let calls = 0;
  let closes = 0;
  const transfer = {
    [Symbol.iterator]() {
      return {
        next() {
          calls++;
          if (phase === 'next' || (phase === 'after-invalid' && calls > 1)) throw expected;
          return {
            get done() { if (phase === 'done') throw expected; return false; },
            get value() { if (phase === 'value') throw expected; return 5; },
          };
        },
        return() { closes++; return { done: true }; },
      };
    },
  };
  try { port1.postMessage({ buffer }, { transfer } as any); }
  catch (error: any) { console.log('throw', phase, error === expected, buffer.byteLength, calls, closes); }
}
const rollback = new ArrayBuffer(4);
try { port1.postMessage({ bad: () => 1, rollback }, new Set([rollback]) as any); }
catch (error: any) { console.log('rollback', error.name, error.code, rollback.byteLength); }

let constructorIterations = 0;
const constructorBuffer = new ArrayBuffer(4);
const constructorTransfer = [constructorBuffer];
constructorTransfer[Symbol.iterator] = function* () { constructorIterations++; throw new Error('ignored'); };
try { new Worker('', { eval: true, workerData: () => 1, transferList: constructorTransfer }); }
catch (error: any) { console.log('constructor', error.name, error.code, constructorBuffer.byteLength, constructorIterations); }
let broadcastReads = 0;
const broadcast = new BroadcastChannel('transfer-normalization');
broadcast.postMessage('value', { get transfer() { broadcastReads++; throw new Error('ignored'); } } as any);
console.log('broadcast', broadcastReads);
broadcast.close();
console.log('queue', receiveMessageOnPort(port2));
port1.close();
port2.close();
