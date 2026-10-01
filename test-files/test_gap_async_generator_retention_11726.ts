// Idle queue metadata must not root the throw closure and its captured payload.
declare function gc(): void;
const canCollect = typeof gc === 'function';
function abandon(start: boolean) {
  const payload = new Array(10000).fill(17);
  const probe = new WeakRef(payload);
  async function* generate() { yield payload.length; yield payload[0]; }
  const iterator = generate();
  if (start) iterator.next();
  return probe;
}
async function main() {
  const unused = abandon(false);
  const suspended = abandon(true);
  await new Promise((resolve) => setTimeout(resolve, 0));
  for (let i = 0; i < 4; i++) {
    if (canCollect) gc();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  console.log('unstarted released', canCollect ? unused.deref() === undefined : true);
  console.log('suspended released', canCollect ? suspended.deref() === undefined : true);
}
main();
