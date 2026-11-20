import { spawn } from 'node:child_process';

const executable = process.env.PERRY_TEST_CHILD_EXECUTABLE;
if (!executable) throw new Error('Set PERRY_TEST_CHILD_EXECUTABLE to a Node executable');
const watchdog = setTimeout(() => { console.error('FAIL: late iterator did not settle'); process.exit(1); }, 5000);
function check(value, message) { if (!value) throw new Error(message); }

async function main() {
  const child = spawn(executable, ['-e', 'process.stdout.write("out"); process.stderr.write("err")'],
    { stdio: ['ignore', 'pipe', 'pipe'] });
  let ends = 0;
  const earlyIterator = child.stdout[Symbol.asyncIterator]();
  for (const stream of [child.stdout, child.stderr]) {
    stream.on('end', () => {
      check(stream.readable === false && stream.readableEnded === true,
        'EOF state must be visible inside the end callback');
      ends++;
    });
    stream.resume();
  }
  await new Promise((resolve, reject) => {
    child.on('error', reject);
    child.on('close', code => code === 0 ? resolve() : reject(new Error('child failed')));
  });
  check(ends === 2, 'both real output pipes must have delivered EOF');
  check((await earlyIterator.next()).done === true, 'first pull after EOF must complete');
  for (const stream of [child.stdout, child.stderr]) {
    let count = 0;
    for await (const _chunk of stream) count++;
    check(count === 0, 'already drained stream must remain empty');
    check((await stream[Symbol.asyncIterator]().next()).done === true,
      'a fresh iterator must also observe retained EOF');
  }
  clearTimeout(watchdog);
  console.log('PASS: child stdout/stderr late async iterators');
}
main().catch(error => { console.error(error.message); process.exit(1); });
