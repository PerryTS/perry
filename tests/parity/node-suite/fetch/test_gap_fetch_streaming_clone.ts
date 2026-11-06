import { fixture, delay } from './fetch_stream_fixture.ts';
async function main() {
  const server = await fixture();
  try {
    const before = Date.now();
    const response = await fetch(server.url + '/clone');
    console.log('clone-headers', Date.now() - before < 180);
    const copy = response.clone();
    const both = await Promise.all([response.text(), copy.text()]);
    console.log('clone-both', both[0], both[0] === both[1]);
    const one = await fetch(server.url + '/one');
    const later = one.clone();
    console.log('clone-one', await one.text());
    console.log('clone-later', await later.text());
    const teed = (await fetch(server.url + '/tee')).body!.tee();
    console.log('tee-both', await new Response(teed[0]).text(), await new Response(teed[1]).text());
    const canceled = (await fetch(server.url + '/tee-cancel')).body!.tee();
    let canceledDone = false;
    const cancellation = canceled[0].cancel().then(() => { canceledDone = true; });
    await delay(30);
    console.log('tee-cancel-waits', !canceledDone);
    console.log('tee-one', await new Response(canceled[1]).text());
    await cancellation;
    console.log('tee-cancel-complete');
    const all = (await fetch(server.url + '/tee-all')).body!.tee();
    await Promise.all([all[0].cancel(), all[1].cancel()]);
    await delay(250);
    const state = (await (await fetch(server.url + '/stats')).json())['/tee-all'];
    console.log('tee-all-disconnect', state.closed, !state.complete, state.sent === 0);
  } finally { await server.close(); }
}
main();
