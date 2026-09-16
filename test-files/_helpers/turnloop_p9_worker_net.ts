// The Worker half of `test_gap_turnloop_p9_worker_agent_net.ts`.
//
// Two files because a Worker whose entry is its own module does not link under
// Perry (turnloop P8's defect 3), which is also how every other worker gap test
// in this tree is shaped.
//
// Each case is posted AS IT FINISHES, never collected and posted at the end.
// The failure this test exists to catch is a hang, and a batch post loses every
// case that DID work to the one that did not -- which is exactly what happened
// the first time this file was written: a third case hung, and the two that had
// already succeeded were never reported, so the run said "WORKER NEVER
// ANSWERED" about a Worker that had answered twice.
import { parentPort, workerData } from 'node:worker_threads';

const { httpPort } = workerData as { httpPort: number };
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

// 1. A fetch as the first thing this agent does.
parentPort?.postMessage(`immediate ${await get('/one')}`);

// 2. The same fetch AFTER this agent has parked on a timer. This is the shape
//    turnloop P8 measured as a hang on both `main` and the integration branch
//    (rc=124 at a 25 s cap): a Worker that parks before fetching never settled
//    its promise. It is here because it is the case a per-agent loop is
//    supposed to answer, and because a hang is the one failure a green suite
//    cannot report.
await new Promise<void>((resolve) => setTimeout(resolve, 30));
parentPort?.postMessage(`after-timer ${await get('/two')}`);
