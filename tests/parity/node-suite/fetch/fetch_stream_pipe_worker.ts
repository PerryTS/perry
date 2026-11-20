import { parentPort } from 'node:worker_threads';
import { Readable, Writable } from 'node:stream';
function createFeed() {
  let blocks: string[] = [], done = false, wake: (() => void) | undefined;
  async function* incoming() {
    for (;;) {
      if (blocks.length) yield blocks.shift()!;
      else if (done) return;
      else await new Promise<void>(resolve => { wake = resolve; });
    }
  }
  return {
    source: incoming(),
    push(block: string) { blocks.push(block); wake?.(); },
    end() { done = true; wake?.(); }
  };
}
let feed: any;
parentPort!.on('message', (message: any) => {
  if (message.start) {
    feed = createFeed();
    let text = '', first = true;
    const output = new Writable({write(chunk, encoding, callback) { text += chunk.toString(); if (first) { first = false; parentPort!.postMessage({first:text}); } callback(); }});
    output.on('finish', () => parentPort!.postMessage({text}));
    Readable.from(feed.source).pipe(output);
  } else if (message.end) feed.end();
  else feed.push(message.block);
});
parentPort!.postMessage('ready');
