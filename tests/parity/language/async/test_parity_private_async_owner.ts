let events = "";
const resolver = Promise.withResolvers<number>();
const gate = resolver.promise;
async function immediateOwner(seed: object): Promise<object> {
  await 0;
  await 1;
  return seed;
}
async function pendingOwner(seed: object, pending: Promise<number>): Promise<object> {
  await pending;
  await 0;
  return seed;
}
async function abruptOwner(seed: object, rejected: Promise<object>): Promise<object> {
  try { await rejected; } catch (error) { await 0; } finally { await 1; }
  return seed;
}
async function checkOwners(): Promise<void> {
  const seed = [17, 29];
  events += "a";
  const first: any = await pendingOwner(seed, gate);
  events += "b";
  const second: any = await immediateOwner(first);
  events += "c";
  const third: any = await abruptOwner(second, Promise.reject(second));
  events += "d";
  console.log(events + ":" + (first === second) + ":" + (third === second) + ":" + (third[0] + third[1]));
}
queueMicrotask(() => {
  events += "q";
  queueMicrotask(() => {
    events += "u";
    queueMicrotask(() => { events += "v"; });
  });
  const noise: object[] = [];
  for (let i = 0; i < 2000; i++) noise.push([i, i + 1]);
  resolver.resolve(noise.length);
});
checkOwners();
