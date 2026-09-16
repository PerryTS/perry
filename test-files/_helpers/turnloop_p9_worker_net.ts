// The Worker half of `test_gap_turnloop_p9_worker_agent_net.ts`.
//
// Two files because a Worker whose entry is its own module does not link under
// Perry (turnloop P8's defect 3), which is also how every other worker gap test
// in this tree is shaped.
import net from 'node:net';
import { parentPort, workerData } from 'node:worker_threads';

const { httpPort, echoPort } = workerData as { httpPort: number; echoPort: number };
const base = `http://127.0.0.1:${httpPort}`;

async function get(path: string): Promise<string> {
  try {
    const res = await fetch(`${base}${path}`);
    const body = await res.text();
    return `status=${res.status} body=${body}`;
  } catch (e) {
    return `error=${(e as Error).message}`;
  }
}

function echo(payload: string): Promise<string> {
  return new Promise<string>((resolve) => {
    const sock = net.connect(echoPort, '127.0.0.1');
    let seen = '';
    sock.on('connect', () => sock.write(payload));
    sock.on('data', (chunk: Buffer) => {
      seen += chunk.toString();
      if (seen.length >= payload.length) sock.end();
    });
    sock.on('close', () => resolve(`echo=${seen}`));
    sock.on('error', (e: Error) => resolve(`error=${e.message}`));
  });
}

const results: string[] = [];

// 1. A fetch as the first thing this agent does.
results.push(`immediate ${await get('/one')}`);

// 2. The same fetch AFTER this agent has parked on a timer. This is the shape
//    turnloop P8 measured as a hang on both `main` and the integration branch
//    (rc=124 at a 25 s cap): a Worker that parks before fetching never settled
//    its promise. It is here because it is the case a per-agent loop is
//    supposed to answer, and because a hang is the one failure a green suite
//    cannot report.
await new Promise<void>((resolve) => setTimeout(resolve, 30));
results.push(`after-timer ${await get('/two')}`);

// 3. A raw socket, which has never gone through fetch's machinery.
results.push(`socket ${await echo('p9-worker')}`);

parentPort?.postMessage(results);
