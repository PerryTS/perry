import { fixture, delay } from './fetch_stream_fixture.ts';
async function main() {
  const server = await fixture();
  try {
    const res = await fetch(server.url + '/cancel');
    const reader = res.body!.getReader();
    await reader.read(); await reader.cancel(); await delay(220);
    const state = (await (await fetch(server.url + '/stats')).json())['/cancel'];
    console.log('reader-cancel', state.closed, !state.complete, state.sent === 9);
    const res2 = await fetch(server.url + '/body-cancel');
    await res2.body!.cancel(); await delay(250);
    const state2 = (await (await fetch(server.url + '/stats')).json())['/body-cancel'];
    console.log('body-cancel', state2.closed, !state2.complete, state2.sent === 0);
    const controller = new AbortController();
    const res3 = await fetch(server.url + '/abort', {signal:controller.signal});
    controller.abort();
    try { await res3.text(); } catch (error: any) { console.log('abort-after-head', error.name); }
    await delay(250);
    const state3 = (await (await fetch(server.url + '/stats')).json())['/abort'];
    console.log('abort-disconnect', state3.closed, !state3.complete, state3.sent === 0);
    const bad = await fetch(server.url + '/error');
    const badReader = bad.body!.getReader();
    console.log('error-progress', !(await badReader.read()).done);
    try { await badReader.read(); } catch (error: any) { console.log('midbody-error', error.name, error.message); }
    try { await (await fetch(server.url + '/error')).text(); } catch (error: any) { console.log('body-error', error.name, error.message); }
  } finally { await server.close(); }
}
main();
