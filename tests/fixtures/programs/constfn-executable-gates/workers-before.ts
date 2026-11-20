import { parallelMap } from 'perry/thread';
import { make, WorkerMade } from './worker-producer.ts';
function invoke(receiver: any) { return receiver.m(); }
// Keep the transferred receiver live through enough allocating worker polls
// for every fixed schedule seed. Verify captures after each possible move.
function collectWhileCalling(receiver: any) {
  const expected = invoke(receiver);
  for (let i = 0; i < 128; i++) {
    const witness = { receiver, tag: 'worker-' + i };
    if (invoke(witness.receiver) !== expected || witness.tag.length < 7) {
      throw new Error('worker capture changed during collection');
    }
  }
  return invoke(receiver);
}
const first = make(17), second = make(29);
const values = parallelMap([first, second], (row: any) => collectWhileCalling(row));
console.log(values[0], values[1], invoke(first), invoke(second));
const constructed = parallelMap([1, 2], (n: number) => collectWhileCalling(new WorkerMade()) + n);
console.log(constructed[0], constructed[1]);
