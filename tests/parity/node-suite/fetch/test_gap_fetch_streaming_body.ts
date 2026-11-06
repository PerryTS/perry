import { fixture, delay } from './fetch_stream_fixture.ts';
async function main() {
  const server = await fixture();
  try {
    const before = Date.now();
    const response = await fetch(server.url + '/drip');
    console.log('headers-before-body', Date.now() - before < 180);
    const reader = response.body!.getReader();
    let count = 0, text = '', first = 0, last = 0;
    const decoder = new TextDecoder();
    while (true) {
      const result = await reader.read();
      if (result.done) break;
      if (count++ === 0) first = Date.now();
      last = Date.now(); text += decoder.decode(result.value);
    }
    console.log('progress', count > 1, last - first >= 160, text);
    console.log('used', response.bodyUsed);
    try { await response.text(); } catch (error: any) { console.log('used-error', error.name); }
    const redir = await fetch(server.url + '/redirect');
    console.log('redirect', redir.status, redir.redirected, redir.url.endsWith('/drip'), await redir.text());
    const redirectedBefore = Date.now();
    const held = await fetch(server.url + '/held-redirect');
    console.log('held-redirect-headers', Date.now() - redirectedBefore < 800, held.redirected, await held.text());
    const redirectStats = (await (await fetch(server.url + '/stats')).json())['/held-redirect'];
    console.log('redirect-before-old-body', !redirectStats.complete, redirectStats.sent === 0);
    console.log('split-headers', await (await fetch(server.url + '/split-head')).text());
    const form = await (await fetch(server.url + '/form')).formData();
    console.log('form', form.get('a'), form.get('n'));
    console.log('json', (await (await fetch(server.url + '/json')).json()).n);
    console.log('bytes', (await (await fetch(server.url + '/bytes')).bytes()).length);
    console.log('arrayBuffer', (await (await fetch(server.url + '/array')).arrayBuffer()).byteLength);
    const blob = await (await fetch(server.url + '/blob')).blob();
    console.log('blob', blob.size, blob.type, await blob.text());
    let iterations = 0, length = 0;
    for await (const chunk of (await fetch(server.url + '/iterator')).body!) { iterations++; length += chunk.length; }
    console.log('iterator', iterations > 1, length);
    const empty = await fetch(server.url + '/null');
    console.log('null', empty.body === null, await empty.text());
    const head = await fetch(server.url + '/head', {method:'HEAD'});
    console.log('HEAD', head.body === null, await head.text());
  } finally { await server.close(); }
}
main();
