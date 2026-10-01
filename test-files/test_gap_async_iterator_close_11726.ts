// MongoDB nests async generators around a per-command event iterator.
const log: string[] = [];
function source(name: string, count: number = 2): any {
  let index = 0;
  return {
    [Symbol.asyncIterator]() { return this; },
    async next() { return { value: ++index, done: index > count }; },
    async return() { await Promise.resolve(); log.push(name + ':close'); return { done: true }; }
  };
}
async function* inner() {
  try { for await (const value of source('source')) { yield value; } }
  finally { log.push('inner:finally'); }
}
async function* outer() {
  try { for await (const value of inner()) { yield value; } }
  finally { log.push('outer:finally'); }
}
async function* awaitedFinalizers() {
  try {
    try { yield 1; }
    finally { await Promise.resolve(); log.push('awaited:inner'); }
  } finally { await Promise.resolve(); log.push('awaited:outer'); }
}
async function earlyReturn() {
  for await (const value of outer()) { return value + 10; }
}
async function main() {
  for await (const value of outer()) { log.push('value:' + value); break; }
  console.log('nested break', log.join(','));
  log.length = 0;
  console.log('nested return', await earlyReturn(), log.join(','));
  log.length = 0;
  const generator = outer();
  console.log('first', JSON.stringify(await generator.next()));
  console.log('external return', JSON.stringify(await generator.return(42 as any)), log.join(','));
  log.length = 0;
  const awaited = awaitedFinalizers();
  await awaited.next();
  console.log('awaited finalizers', JSON.stringify(await awaited.return(23 as any)), log.join(','));
  log.length = 0;
  for await (const value of source('natural')) { log.push('value:' + value); }
  console.log('natural', log.join(','));
  log.length = 0;
  for await (const value of source('continue')) {
    if (value === 1) continue;
    log.push('value:' + value);
  }
  console.log('continue', log.join(','));
  log.length = 0;
  for await (const value of source('nested break', 1)) {
    for (let i = 0; i < 2; i++) { break; }
    log.push('value:' + value);
  }
  console.log('inner loop break', log.join(','));
  log.length = 0;
  outside: for (let i = 0; i < 2; i++) {
    for await (const value of source('label')) { log.push('value:' + value); continue outside; }
  }
  console.log('outer continue', log.join(','));
  log.length = 0;
  const throwing = source('throw');
  throwing.return = async function() { log.push('throw:close'); throw new Error('close error'); };
  try { for await (const value of throwing) { throw new Error('body error'); } }
  catch (e: any) { log.push(e.message); }
  console.log('throw precedence', log.join(','));
  log.length = 0;
  const rejecting = source('next error');
  rejecting.next = async function() { throw new Error('next error'); };
  try { for await (const value of rejecting) { log.push('unexpected'); } }
  catch (e: any) { log.push(e.message); }
  console.log('failed next', log.join(','));
  log.length = 0;
  const noReturn: any = { [Symbol.asyncIterator]() { return this; }, async next() { return { value: 7, done: false }; } };
  for await (const value of noReturn) { log.push('value:' + value); break; }
  console.log('no return', log.join(','));
  let gets = 0;
  const getter = source('getter');
  Object.defineProperty(getter, 'return', { get() {
    gets++;
    return async function(this: any) { log.push('receiver:' + (this === getter)); return { done: true }; };
  } });
  log.length = 0;
  for await (const value of getter) { break; }
  console.log('return getter', gets, log.join(','));
}
main().catch((e: any) => console.log('ERROR', e.message));
