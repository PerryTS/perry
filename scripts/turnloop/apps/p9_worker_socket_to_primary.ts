// A defect this lane found and did NOT fix, kept as a reproducer so the report
// is checkable.
//
// A raw `net.connect` from a worker agent to a `net.createServer` listener
// owned by the PRIMARY agent of the SAME process hangs: the connection is made,
// the write is submitted, and no 'data' ever arrives. The same worker socket to
// an OUT-OF-PROCESS server works (`p9_worker_agent_acceptance.ts` does exactly
// that and gets its echo back), and a `fetch` from the same worker to the same
// process's own `http.createServer` works too -- so it is specific to the raw
// socket path crossing agents inside one process.
//
// It is not a P9 regression in the usual sense: on the base commit a worker
// agent is refused a loop and its `net.connect` returns NO DATA AT ALL, to any
// server, in or out of process. So this shape has never worked. What P9 changed
// is that it now reaches turnloop instead of failing in tokio.
//
//   P9_AGENTS=1 ./p9_worker_socket_to_primary   # hangs -> exit 3 at the watchdog
//
// The watchdog is what makes it a report rather than a stall.
import net from 'node:net';
import { Worker } from 'node:worker_threads';

const agents = Number(process.env.P9_AGENTS ?? '1');
const budgetMs = Number(process.env.P9_BUDGET_MS ?? '8000');

const echoServer = net.createServer((sock) => {
  sock.on('data', (chunk: unknown) => sock.write(chunk as string));
  sock.on('error', () => {});
});

const echoPort = await new Promise<number>((resolve) => {
  echoServer.listen(0, '127.0.0.1', () => {
    resolve((echoServer.address() as { port: number }).port);
  });
});

const workerUrl = new URL('./_helpers/p9_socket_concurrency_worker.ts', import.meta.url);
const answers: string[] = [];
for (let i = 0; i < agents; i++) {
  const w = new Worker(workerUrl, { workerData: { index: i, echoPort, budgetMs } });
  w.on('message', (m: unknown) => answers.push(String(m)));
  w.on('error', (e: Error) => answers.push('worker-error:' + e.message));
}

const deadline = Date.now() + budgetMs + 2000;
while (answers.length < agents && Date.now() < deadline) {
  await new Promise<void>((r) => setTimeout(r, 25));
}

const ok = answers.filter((a) => a === 'ok').length;
console.log(`in-process echo: agents=${agents} answered=${answers.length}/${agents} ok=${ok}/${agents} ${answers.join(",")}`);
process.exit(ok === agents ? 0 : 3);
