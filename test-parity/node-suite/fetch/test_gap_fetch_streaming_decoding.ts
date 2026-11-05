import { fixture } from './fetch_stream_fixture.ts';
async function main() {
  const server = await fixture();
  try {
    for (const encoding of ['gzip','br','deflate']) {
      const before = Date.now();
      const response = await fetch(server.url + '/' + encoding);
      const headersEarly = Date.now() - before < 180;
      const reader = response.body!.getReader();
      let chunks = 0, text = '', first = 0, last = 0;
      const decoder = new TextDecoder();
      while (true) {
        const result = await reader.read(); if (result.done) break;
        if (chunks++ === 0) first = Date.now();
        last = Date.now(); text += decoder.decode(result.value);
      }
      console.log(encoding, headersEarly, chunks > 1, last - first >= 160, text);
    }
    console.log('gzip-short', await (await fetch(server.url + '/gzip-short')).text());
    const bad = await fetch(server.url + '/gzip-bad');
    try { await bad.text(); } catch (error: any) { console.log('gzip-error', error.name, error.message); }
  } finally { await server.close(); }
}
main();
