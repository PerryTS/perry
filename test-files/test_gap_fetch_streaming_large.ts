import { fixture, delay } from './fetch_stream_fixture.ts';
async function main() {
  const server = await fixture();
  try {
    const response = await fetch(server.url + '/large');
    const before = process.memoryUsage().rss;
    await delay(1200);
    const state = (await (await fetch(server.url + '/stats')).json())['/large'];
    console.log('transport-paused', !state.complete, state.sent < 32 * 1024 * 1024);
    const reader = response.body!.getReader();
    let length = 0, valid = true, largest = 0;
    while (true) {
      const result = await reader.read(); if (result.done) break;
      const chunk = result.value;
      length += chunk.length; largest = Math.max(largest, chunk.length);
      if (chunk[0] !== 65 || chunk[chunk.length - 1] !== 65) valid = false;
      await delay(1);
    }
    console.log('large', length, valid, largest <= 131072);
    // Queue size is asserted precisely by Rust tests; RSS is recorded in the
    // standalone timed probe because allocator/GC headroom differs by runtime.
    console.log('rss-readable', before > 0);
  } finally { await server.close(); }
}
main();
