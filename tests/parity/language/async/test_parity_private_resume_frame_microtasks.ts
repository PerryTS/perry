const events: string[] = [];
async function task() {
  events.push('start');
  await Promise.resolve(1);
  events.push('first');
  await 2;
  events.push('second');
}
const promise = task();
queueMicrotask(() => events.push('queued'));
promise.then(() => { events.push('then'); console.log(events.join(',')); });
