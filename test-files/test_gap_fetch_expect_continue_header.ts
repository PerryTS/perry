// `Expect` is a forbidden request header for `fetch`, and undici does not
// silently drop it — it refuses to build the request, so `fetch()` rejects with
// `TypeError: fetch failed` whose cause is
// `NotSupportedError: expect header not supported` (`UND_ERR_NOT_SUPPORTED`).
//
// Perry gave three different answers to that one program, none of them Node's,
// and which one you got depended on the transport:
//
//   * reqwest — the header went on the wire and the request SUCCEEDED.
//   * the turnloop engine — the head reached `Http1Connection::start`, which
//     reads `expect: 100-continue` on a non-empty body as "park the upload
//     until the server answers 100". `can_send_body()` then returns false and
//     the very next `send_body` fails `UND_ERR_INVALID_ARG "request body is not
//     writable"`, so the POST never left the process and the rejection named
//     the BODY rather than the header.
//   * an empty body — `waiting_continue` is not set, so that arm behaved like
//     the reqwest one.
//
// A test that only checked "does it reject" would have passed on the turnloop
// arm for the wrong reason, so this asserts the cause's name, message and code,
// and covers the empty-body arm that would otherwise hide half a fix.
import http from 'node:http';

const server = http.createServer((req, res) => {
  let body = '';
  req.on('data', (chunk: any) => {
    body += chunk.toString();
  });
  req.on('end', () => {
    res.writeHead(200, { 'content-type': 'text/plain' });
    res.end('expect=' + (req.headers['expect'] ?? '<absent>') + ' body=' + body);
  });
});

async function report(label: string, go: () => Promise<Response>): Promise<void> {
  try {
    const response = await go();
    console.log(label + ' -> ' + response.status + ' ' + (await response.text()));
  } catch (error: any) {
    const cause = error.cause;
    console.log(
      label + ' -> ' + error.name + ' ' + JSON.stringify(error.message) +
      ' cause: ' + cause?.name + ' ' + JSON.stringify(cause?.message) +
      ' code=' + cause?.code
    );
  }
}

async function main(): Promise<void> {
  await new Promise<void>((resolve) => {
    server.listen(0, '127.0.0.1', () => resolve());
  });
  const port = (server.address() as any).port;
  const base = 'http://127.0.0.1:' + port;

  // The shape the turnloop engine could not send at all.
  await report('literal header, with body', () =>
    fetch(base + '/a', {
      method: 'POST',
      body: 'payload-one',
      headers: { Expect: '100-continue' },
    }));

  // Lower-case, and through a `Headers` object rather than an object literal.
  const headers = new Headers();
  headers.set('expect', '100-continue');
  headers.set('x-keep', 'yes');
  await report('Headers object, with body', () =>
    fetch(base + '/b', { method: 'PUT', body: 'payload-two', headers }));

  // No body: `waiting_continue` was never set here, so this arm behaved
  // differently from the two above. A fix that only handled the non-empty case
  // would still pass them and fail this.
  await report('no body', () =>
    fetch(base + '/c', { method: 'POST', headers: { Expect: '100-continue' } }));

  // An Expect value that is not 100-continue is refused just the same: undici
  // rejects the header by NAME.
  await report('non-continue value', () =>
    fetch(base + '/d', {
      method: 'POST',
      body: 'payload-four',
      headers: { expect: 'something-else' },
    }));

  // The control: no Expect, so it must still go out and come back.
  await report('control, no Expect', () =>
    fetch(base + '/e', { method: 'POST', body: 'payload-five' }));

  server.close();
}

main();
