// turnloop P6 acceptance: an abort that lands MID-BODY.
//
// The server sends the head and part of the body, then stalls. The client
// aborts while the response is still being read, so the cancellation has to
// reach a socket with an outstanding multishot read and an in-flight request —
// the case that distinguishes "the promise rejected" from "the operation was
// actually cancelled on the loop, exactly once".
//
// The second half is the assertion that makes it worth running: after the
// abort, the SAME loop must still serve a normal request. A cancellation that
// left the engine's tables inconsistent, double-released a pool slot or leaked
// a turnloop handle would show up there rather than in the rejection.
import http from 'node:http';

let stalled: http.ServerResponse | null = null;

const server = http.createServer((req, res) => {
  if (req.url === '/stall') {
    res.setHeader('content-type', 'text/plain');
    res.setHeader('content-length', '1000');
    res.write('x'.repeat(100));
    stalled = res;
    return;
  }
  res.setHeader('content-type', 'text/plain');
  res.end('after-abort-ok');
});

function listen(): Promise<number> {
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      resolve(typeof address === 'object' && address !== null ? address.port : 0);
    });
  });
}

async function main() {
  const port = await listen();
  const base = `http://127.0.0.1:${port}`;

  // Where "mid-body" falls differs between the engines, so BOTH shapes are
  // exercised. Node's fetch resolves at the head and streams the body, so an
  // abort after the head has to reject `res.text()`. Perry's buffers the whole
  // body before resolving, so the same abort lands inside the `fetch()` await.
  // Printing both means neither engine can pass by accident.
  const controller = new AbortController();
  const pending = fetch(`${base}/stall`, { signal: controller.signal });
  let head: Response | null = null;
  const headOrNull: Promise<Response | null> = pending.then(
    (r) => { head = r; return r; },
    () => null,
  );

  // Abort once the head and the first body bytes are definitely on the wire.
  await new Promise((r) => setTimeout(r, 150));
  console.log('server-stalled', stalled !== null);
  controller.abort();

  try {
    await pending;
    console.log('mid-body abort (fetch)', 'NOT-REJECTED');
  } catch (err) {
    console.log('mid-body abort (fetch)', (err as Error).name);
  }
  await headOrNull;
  if (head !== null) {
    try {
      await (head as Response).text();
      console.log('mid-body abort (body)', 'NOT-REJECTED');
    } catch (err) {
      console.log('mid-body abort (body)', (err as Error).name);
    }
  } else {
    console.log('mid-body abort (body)', 'n/a — the fetch itself rejected');
  }

  // Aborting again must not produce a second settlement or a crash.
  controller.abort();

  // The loop is still healthy.
  const after = await fetch(`${base}/plain`);
  console.log('after-abort status', after.status, await after.text());

  // And so is the pool: several more requests on the same origin.
  const more = await Promise.all(
    [1, 2, 3].map(() => fetch(`${base}/plain`).then((r) => r.status)),
  );
  console.log('after-abort concurrent', more.join(','));

  if (stalled) {
    try { stalled.end('y'.repeat(900)); } catch {}
  }
  server.close();
  process.exit(0);
}

main();
