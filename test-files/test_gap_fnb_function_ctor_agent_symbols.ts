import { Worker, isMainThread, parentPort } from 'node:worker_threads';
const localKey = Symbol('per-agent'), registryKey = Symbol.for('FNB-agent');
function Body(this: any, value: number) { this.first = value; this[localKey] = value + 1; this[registryKey] = value + 2; }
function exercise() {
  const original = Body.prototype;
  const a = new Body(1);
  Body.prototype = { changed: true };
  const b = new Body(4);
  function factory() {
    let key = localKey;
    const Captured = function(this: any) { this.first = 1; this[key] = 2; };
    return { Captured, change() { key = registryKey; } };
  }
  const f = factory(), c = new f.Captured(); f.change(); const d = new f.Captured();
  return [a[localKey], b[registryKey], Object.getPrototypeOf(a) === original,
    Object.getPrototypeOf(b) === Body.prototype, c[localKey], d[registryKey],
    Object.getOwnPropertySymbols(a).map(k => k === localKey ? 'local' : 'registry').join(',')].join('|');
}
if (isMainThread) {
  // Let two agents materialize the shared descriptor before the main agent.
  let completed = 0;
  const values: string[] = [], exits: number[] = [];
  for (let i = 0; i < 2; i++) {
    const w = new Worker(new URL(import.meta.url));
    w.on('message', value => { values[i] = value; });
    w.on('exit', code => {
      exits[i] = code;
      if (++completed === 2) {
        console.log('workers', values.join(';'));
        console.log('exits', exits.join(','));
        console.log('main', exercise());
      }
    });
  }
} else {
  parentPort!.postMessage(exercise());
}
