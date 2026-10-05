// Stored fields cannot all be observed on the wire. Request must see the same
// dictionary conversion as fetch, including helper/spread/copy overrides.
function options(): any {
  return { method: 'POST', body: 'body', headers: [['x-test', 'kept']],
    keepalive: true, duplex: 'half', redirect: 'manual', cache: 'no-store',
    credentials: 'include', mode: 'same-origin', integrity: 'sha256-example',
    referrer: 'https://example.com/', referrerPolicy: 'no-referrer' };
}
async function main() {
  const url = 'https://example.com/';
  const opts = options();
  for (const init of [opts, { ...opts }, options()]) {
    const req = new Request(url, init);
    console.log(req.method, req.headers.get('x-test'), req.keepalive, req.duplex,
      req.redirect, req.cache, req.credentials, req.mode, req.integrity,
      req.referrer, req.referrerPolicy, await req.text());
  }
  const req = new Request(url, opts);
  const copy = new Request(req, { ...opts, body: 'copy', keepalive: false, redirect: 'error' });
  console.log('copy', copy.keepalive, copy.duplex, copy.redirect, await copy.text(), req.bodyUsed);
  for (const keepalive of [false, true, 0, 1, '', 'yes', NaN, null]) {
    console.log('keepalive-conversion', new Request(url, { keepalive } as any).keepalive);
  }
  const prototype = { method: 'PUT', body: 'inherited', headers: { 'x-test': 'prototype' } };
  const inherited = Object.create(prototype);
  const fromPrototype = new Request(url, inherited);
  console.log('prototype', fromPrototype.method, fromPrototype.headers.get('x-test'),
    await fromPrototype.text());
}
main();
