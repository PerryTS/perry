// parity-env: NODE_TLS_REJECT_UNAUTHORIZED=0 NODE_NO_WARNINGS=1
// parity-node-argv: --no-warnings
import { fixture, delay } from './fetch_stream_fixture.ts';
process.env.NODE_TLS_REJECT_UNAUTHORIZED = '0';
async function main() {
  const server = await fixture(true);
  try {
    const before = Date.now();
    const response = await fetch(server.url + '/tls');
    console.log('tls-headers', Date.now() - before < 180);
    let chunks = 0, text = '';
    for await (const chunk of response.body!) { chunks++; text += new TextDecoder().decode(chunk); }
    console.log('tls-progress', chunks > 1, text);
    const reader = (await fetch(server.url + '/tls-cancel')).body!.getReader();
    await reader.read(); await reader.cancel(); await delay(250);
    const state = (await (await fetch(server.url + '/stats')).json())['/tls-cancel'];
    console.log('tls-disconnect', state.closed, !state.complete, state.sent === 9);
    console.log('tls-split', await (await fetch(server.url + '/split-head')).text());
  } finally { await server.close(); }
}
main();
