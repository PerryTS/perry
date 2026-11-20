// Every call shape must retain the whole evaluated init. Local echo server
// avoids registry/network drift; print only application-controlled fields.
import { createServer } from 'node:http';

function makeOptions(headers: any): any {
  return { method: 'post', body: 'payload', headers, keepalive: true, duplex: 'half' };
}
function forwarded(url: string, init: any): Promise<Response> {
  return fetch(url, init);
}
async function main() {
  const server = createServer((req, res) => {
    let body = '';
    req.on('data', (chunk) => { body += chunk.toString(); });
    req.on('end', () => {
      res.end(JSON.stringify([req.method, body, req.headers.accept,
        req.headers['x-value'], req.headers['content-type']]));
    });
  });
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  const url = 'http://127.0.0.1:' + (server.address() as any).port;
  const plain = { Accept: 'application/vnd.npm.install-v1+json', 'X-Value': 'one' };
  const pairs = [['Accept', plain.Accept], ['X-Value', 'one']];
  const headers = new Headers(pairs);
  for (const headerForm of [plain, pairs, headers]) {
    const opts = makeOptions(headerForm);
    console.log('variable', await (await fetch(url, opts)).text());
    console.log('helper', await (await fetch(url, makeOptions(headerForm))).text());
    console.log('forwarded', await (await forwarded(url, opts)).text());
    console.log('spread', await (await fetch(url, { ...opts })).text());
    console.log('assign', await (await fetch(url, Object.assign({}, opts))).text());
    const runtime: any = {};
    runtime.method = 'post'; runtime.body = 'payload'; runtime.headers = headerForm;
    console.log('built', await (await fetch(url, runtime)).text());
    console.log('literal', await (await fetch(url, {
      method: 'post', body: 'payload', headers: headerForm,
    })).text());
    const alias = globalThis.fetch;
    console.log('alias', await (await alias(url, opts)).text());
    console.log('arguments', await (await fetch(...([url, opts] as [string, any]))).text());
    const request = new Request(url, opts);
    request.headers.set('x-value', 'mutated');
    console.log('request', await (await fetch(request)).text());
    console.log('request-used', request.bodyUsed);
    const original = new Request(url, opts);
    const override = { method: 'PUT', body: 'override', headers: { 'X-Value': 'new' } };
    console.log('override', await (await fetch(original, override)).text());
    console.log('original-used', original.bodyUsed);
  }
  const requestInit = new Request(url, { headers: plain });
  console.log('request-as-init', await (await fetch(url, requestInit as any)).text());
  console.log('json', await (await fetch(url, JSON.parse(JSON.stringify(makeOptions(plain))))).text());
  console.log('none', await (await fetch(url)).text());
  console.log('undefined', await (await fetch(url, undefined)).text());
  console.log('null', await (await fetch(url, null as any)).text());
  let calls = 0;
  const key = 'headers';
  console.log('computed', await (await fetch(url, {
    method: 'POST', body: 'payload', [key]: plain,
    ignored: ++calls,
  } as any)).text(), calls);
  const reads: string[] = [];
  const getters: any = {
    get method() { reads.push('method'); return 'POST'; },
    get headers() { reads.push('headers'); return plain; },
    get body() { reads.push('body'); return 'payload'; },
    get keepalive() { reads.push('keepalive'); return true; },
    get duplex() { reads.push('duplex'); return 'half'; },
  };
  console.log('extra', await (await fetch(url, makeOptions(plain), (++calls as any))).text(), calls);
  console.log('literal-getter', await (await fetch(url, {
    method: 'POST', body: 'payload', get headers() { return plain; },
  })).text());
  console.log('getters', await (await fetch(url, getters)).text());
  console.log('reads', reads.join(','));
  const savedFetch = globalThis.fetch;
  globalThis.fetch = async () => ({ text: async () => 'mock' } as any);
  console.log('replaced-global', await (await fetch(url, makeOptions(plain))).text());
  globalThis.fetch = savedFetch;
  await new Promise<void>(resolve => server.close(() => resolve()));
}
main();
