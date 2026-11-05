import { Worker, isMainThread, parentPort, postMessageToThread, threadId, workerData } from 'node:worker_threads';

const missing = 2147483647;
async function rejected(label: string, target: number, kind: string) {
  const buffer = new ArrayBuffer(4);
  const sentinel = new Error('not-read');
  let reads = 0;
  let returned = false;
  const value = kind === 'value' ? { get payload() { reads++; throw sentinel; } } : { buffer };
  const transfer = kind === 'transfer' ? { get transfer() { reads++; throw sentinel; } } : [buffer];
  try {
    const promise = postMessageToThread(target, value, transfer as any, 1000);
    returned = true;
    await promise;
    console.log(label, 'resolved');
  } catch (error: any) {
    console.log(label, error.code ?? '', buffer.byteLength, reads, returned);
  }
}
function nextMessage(worker: Worker, phase: string): Promise<any> {
  return new Promise(resolve => worker.on('message', message => {
    if (message.phase === phase) resolve(message);
  }));
}
async function main() {
  for (const kind of ['array', 'transfer', 'value']) await rejected(`missing-${kind}`, missing, kind);
  for (const kind of ['array', 'transfer', 'value']) await rejected(`same-${kind}`, threadId, kind);
  const live = new Worker(new URL(import.meta.url), { workerData: 'live' });
  let stopped = false;
  try {
    await nextMessage(live, 'ready');
    const target = live.threadId;
    const received = nextMessage(live, 'delivered');
    const buffer = new ArrayBuffer(4);
    new Uint8Array(buffer)[0] = 7;
    await postMessageToThread(target, { buffer }, [buffer], 1000);
    const packet = await received;
    console.log('live', buffer.byteLength, packet.length, packet.first, packet.source === threadId);
    await rejected('unrelated-missing', missing, 'array');
    const count = nextMessage(live, 'count');
    live.postMessage('count');
    console.log('delivery-count', (await count).received);
    console.log('terminated', await live.terminate());
    stopped = true;
    for (const kind of ['array', 'transfer', 'value']) await rejected(`dead-${kind}`, target, kind);
  } finally {
    if (!stopped) await live.terminate();
  }
  const origin = new Worker(new URL(import.meta.url), { workerData: 'origin' });
  try {
    const result = await nextMessage(origin, 'origin');
    console.log('worker-origin', result.code, result.length);
  } finally {
    await origin.terminate();
  }
}
if (isMainThread) {
  main().catch(error => console.log('unexpected', error.name, error.code ?? ''));
} else {
  setInterval(() => {}, 1000);
  parentPort!.on('message', () => {});
  if (workerData === 'live') {
    let received = 0;
    process.on('workerMessage', (value: any, source: number) => {
      received++;
      parentPort!.postMessage({ phase: 'delivered', length: value.buffer.byteLength, first: new Uint8Array(value.buffer)[0], source });
    });
    parentPort!.on('message', () => parentPort!.postMessage({ phase: 'count', received }));
    parentPort!.postMessage({ phase: 'ready' });
  } else {
    const buffer = new ArrayBuffer(4);
    postMessageToThread(missing, { buffer }, [buffer], 1000).catch(error => {
      parentPort!.postMessage({ phase: 'origin', code: error.code, length: buffer.byteLength });
    });
  }
}
