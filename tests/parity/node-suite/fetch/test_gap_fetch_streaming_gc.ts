// parity-node-argv: --expose-gc
declare function gc(): void;
import { fixture, delay } from './fetch_stream_fixture.ts';
async function main() {
  const server = await fixture();
  try {
    const response = await fetch(server.url + '/gc');
    await delay(450);
    const reader = response.body!.getReader();
    const decoder = new TextDecoder();
    let text = '', count = 0;
    while (true) {
      const result = await reader.read();
      if (result.done) break;
      const chunk = result.value;
      // Chunks remaining in the native stream queue, and this chunk retained
      // across callbacks, must survive copying and full sweep collections.
      let total = 0;
      for (let i = 0; i < 600; i++) {
        const temporary = new Array(128).fill('root-' + i);
        total += temporary[0].length;
      }
      if (typeof gc === 'function') gc();
      await delay(20);
      text += decoder.decode(chunk);
      count++;
      if (total === 0) console.log('impossible');
    }
    console.log('chunk-roots', count > 1, text);
    const closing = (await fetch(server.url + '/gc-close')).body!.getReader();
    const reads = await Promise.all(Array.from({length:8}, () => closing.read()));
    console.log('close-read-roots', reads.some(x => x.done),
      reads.reduce((n, x) => n + (x.done ? 0 : x.value.length), 0));
    try {
      const failing = (await fetch(server.url + '/error')).body!.getReader();
      const outcomes = await Promise.allSettled(Array.from({length:4}, () => failing.read()));
      const rejected = outcomes.filter(x => x.status === 'rejected');
      console.log('error-read-roots', outcomes.some(x => x.status === 'fulfilled'),
        rejected.length > 0 && rejected.every((x:any) => x.reason.name === 'TypeError' && x.reason.message === 'terminated'));
    } catch {
      console.log('error-read-roots', 'no-head');
    }
  } finally { await server.close(); }
}
main();
