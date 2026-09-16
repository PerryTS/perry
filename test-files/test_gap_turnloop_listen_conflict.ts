// Two `http.createServer()`s, one port. Node answers EADDRINUSE on the second.
//
// This is the test that would have caught the bug it pins: `turnloop_serve`
// passed `server.noDelay` (default true) into `tcp_listen`'s `reuse_port`
// parameter, so every turnloop HTTP listener bound with SO_REUSEPORT and the
// second bind quietly succeeded. Nothing on that path wants SO_REUSEPORT — the
// cluster worker that does declines the turnloop path before it gets here.
//
// The port is ephemeral, not a literal: this file runs inside a sharded suite
// and once per arm of an A/B, so a fixed port would collide with itself. The
// number never reaches stdout, only decisions about it do.
import http from 'node:http';

function listenEphemeral(server: any): Promise<number> {
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => resolve((server.address() as any).port));
  });
}

function attempt(label: string, port: number): Promise<string> {
  return new Promise((resolve) => {
    const server = http.createServer((_req, res) => res.end('ok'));
    let settled = false;
    const done = (outcome: string) => {
      if (settled) return;
      settled = true;
      resolve(outcome);
    };
    server.on('error', (err: any) => done(`${label}: error ${err.code}`));
    server.listen(port, '127.0.0.1', () => {
      const address = server.address() as any;
      done(`${label}: listening port-matches=${address && address.port === port}`);
      server.close();
    });
  });
}

async function main() {
  const first = http.createServer((_req, res) => res.end('first'));
  const port = await listenEphemeral(first);
  console.log('first: listening');

  // The same port, while the first server still holds it.
  console.log(await attempt('second', port));
  // A second attempt fails the same way: the first failure must not have
  // released anything.
  console.log(await attempt('third', port));
  // The original server is untouched by either failure.
  console.log('first still listening:', first.listening);

  first.close();
  console.log('done');
}

main();
