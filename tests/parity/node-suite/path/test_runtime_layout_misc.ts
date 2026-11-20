import { posix, win32 } from 'node:path';
import { performance } from 'node:perf_hooks';
function check(record: any) {
  console.log(Object.keys(record).join(','));
  console.log(Object.getPrototypeOf(record) === Object.prototype);
  for (const key of keys) {
    const d = Object.getOwnPropertyDescriptor(record, key)!;
    console.log(key, d.writable, d.enumerable, d.configurable);
  }
  record.extra = 31;
  console.log(Object.keys(record).slice(-1).join(','), record.extra);
}
check(posix.parse('/a/b.txt'));
check(win32.parse('C:/a/b.txt'));
const data: any = {field: 3};
Object.defineProperty(data, 'accessor', {get() { return 8; }, configurable: true});
check(Object.getOwnPropertyDescriptor(data, 'field'));
check(Object.getOwnPropertyDescriptor(data, 'accessor'));
check(process.cpuUsage());
check(process.memoryUsage());
check(performance.eventLoopUtilization());
// Preserve the checked Perry order; this aggregate already differs from Node.
check(performance.toJSON(), [eventLoopUtilization, nodeTiming, timeOrigin]);
check(performance.nodeTiming.toJSON());
const cap = Promise.withResolvers();
check(cap);
cap.resolve(9);
Promise.allSettled([Promise.resolve(3), Promise.reject(4)]).then((records: any[]) => {
  check(records[0]);
  check(records[1]);
  console.log(records[0].status, records[0].value, records[1].status, records[1].reason);
});
