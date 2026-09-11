import { spawn } from 'node:child_process';
import { setTimeout as delay } from 'node:timers/promises';

const watchdog = setTimeout(() => { console.error('FAIL: failed child collectors stalled'); process.exit(1); }, 5000);
function check(value, message) { if (!value) throw new Error(message); }
async function collect(stream) {
  let bytes = 0;
  for await (const chunk of stream) bytes += chunk.length;
  return bytes;
}
async function main() {
  const missing = process.env.PERRY_TEST_MISSING_CHILD;
  check(typeof missing === 'string', 'runner must supply a definitely absent executable');
  const child = spawn(missing, [], { stdio: ['ignore', 'pipe', 'pipe', 'pipe'] });
  const streams = [child.stdout, child.stderr, child.stdio[3]];
  let errorSeen = false, ends = 0;
  for (const stream of streams) stream.on('end', () => {
    check(errorSeen, 'failed spawn must emit its child error before stream end');
    ends++;
  });
  const readers = streams.map(collect);
  const closed = new Promise(resolve => child.on('close', resolve));
  const failure = new Promise(resolve => child.on('error', error => {
    errorSeen = true;
    resolve(error.code);
  }));
  // Make a prematurely armed close timer overdue before yielding. Error must
  // still precede pipe EOF, irrespective of native optimization/startup speed.
  const until = Date.now() + 20;
  while (Date.now() < until) {}
  const code = await failure;
  check(code === 'ENOENT', 'must exercise the real OS spawn failure');
  // Execa-shaped cleanup: yield, destroy output streams, then await collectors.
  await delay(0);
  for (const stream of streams) stream.destroy();
  const results = await Promise.all(readers);
  await closed;
  check(ends === 3 && results.every(bytes => bytes === 0), 'all three empty output pipes must end');
  for (const stream of streams) {
    check(stream.readable === false && stream.readableEnded === true &&
      stream.destroyed === true && stream.closed === true, 'failed output must be terminal');
    check((await stream[Symbol.asyncIterator]().next()).done === true, 'late failed-output reader must finish');
  }
  clearTimeout(watchdog);
  console.log('PASS: failed child output collectors');
}
main().catch(error => { console.error(error.message); process.exit(1); });
