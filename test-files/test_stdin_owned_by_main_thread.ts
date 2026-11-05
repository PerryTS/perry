// fd 0 belongs to the main thread: a Worker's `process.stdin` is not the
// terminal (Node). Perry's stdin buffer and listener lists are process-global,
// and every agent's event loop drained them, so a busy worker swallowed the
// main thread's input and ran the main thread's `data` listener on the worker
// thread (a TUI lost keystrokes and terminal replies at random).
//
// Undriven it prints `RESULT:idle` without touching stdin.
// `crates/perry/tests/stdin_owned_by_main_thread.rs` pipes bytes in.
import { Worker } from 'node:worker_threads';

if (process.env.PERRY_STDIN_OWNER_DRIVE !== '1') {
  console.log('RESULT:idle');
} else {
  const worker = new Worker(new URL('./_helpers/stdin_owner_spin_worker.ts', import.meta.url));
  await new Promise<void>((resolve) => worker.once('message', () => resolve()));
  let total = 0;
  const onData = (chunk: { length: number }) => {
    total += chunk.length;
  };
  const onEnd = () => {
    worker.postMessage('stop');
    console.log(`RESULT:${total}`);
  };
  if (process.env.PERRY_STDIN_OWNER_SPELLING === 'literal') {
    process.stdin.on('data', onData);
    process.stdin.on('end', onEnd);
  } else {
    // An aliased stream, as a TUI holds it (`this.stdin.on("data", ...)`).
    const input: any = process.stdin;
    input.on('data', onData);
    input.on('end', onEnd);
  }
  console.log('READY');
}
