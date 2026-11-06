// Node-differential parity test for AsyncLocalStorage / async_hooks context
// propagation (#788/#789). Byte-compared against `node` (v26+); every case is
// deterministic (no wall-clock-dependent cross-timer ordering).
import { AsyncLocalStorage, AsyncResource } from 'node:async_hooks';

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function caseBasicAwait() {
  console.log('== basic-await ==');
  const als = new AsyncLocalStorage<{ id: number }>();
  await als.run({ id: 1 }, async () => {
    await Promise.resolve();
    console.log('after-await', als.getStore()?.id);
    await sleep(1);
    console.log('after-timer-await', als.getStore()?.id);
  });
  console.log('outside', als.getStore());
}

async function caseNestedRun() {
  console.log('== nested-run ==');
  const als = new AsyncLocalStorage<string>();
  als.run('outer', () => {
    console.log('a', als.getStore());
    als.run('inner', () => console.log('b', als.getStore()));
    console.log('c', als.getStore());
  });
  console.log('d', als.getStore());
}

async function caseEnterWithInRun() {
  console.log('== enterwith-in-run ==');
  const als = new AsyncLocalStorage<string>();
  als.run('r', () => {
    console.log('1', als.getStore());
    als.enterWith('e');
    console.log('2', als.getStore());
  });
  // run() restores its own storage's pre-run value even after enterWith.
  console.log('3', als.getStore());
}

async function caseCrossStorage() {
  console.log('== cross-storage ==');
  const a = new AsyncLocalStorage<string>();
  const b = new AsyncLocalStorage<string>();
  a.run('A', () => {
    b.enterWith('B');
    console.log('1', a.getStore(), b.getStore());
  });
  // a.run restores only its own storage; b's enterWith survives.
  console.log('2', a.getStore(), b.getStore());
  b.run('B2', () => {
    a.exit(() => {
      b.enterWith('B3');
      console.log('3', a.getStore(), b.getStore());
    });
    console.log('4', a.getStore(), b.getStore());
  });
  console.log('5', a.getStore(), b.getStore());
}

async function caseThrowRestores() {
  console.log('== throw-restores ==');
  const als = new AsyncLocalStorage<string>();
  als.run('outer', () => {
    try {
      als.run('inner', () => {
        throw new Error('boom');
      });
    } catch (e: any) {
      console.log('caught', e.message, 'store', als.getStore());
    }
  });
  console.log('end', als.getStore());
}

async function caseResourceThrow() {
  console.log('== resource-throw ==');
  const als = new AsyncLocalStorage<string>();
  const res = new AsyncResource('T');
  als.run('outer', () => {
    try {
      res.runInAsyncScope(() => {
        throw new Error('rs');
      });
    } catch (e: any) {
      console.log('caught', e.message, als.getStore());
    }
  });
  console.log('end', als.getStore());
}

async function caseMicrotaskOrdering() {
  console.log('== microtask-ordering ==');
  const als = new AsyncLocalStorage<string>();
  await new Promise<void>((done) => {
    als.run('m', () => {
      queueMicrotask(() => console.log('qm', als.getStore()));
      process.nextTick(() => {
        console.log('nt', als.getStore());
        done();
      });
      Promise.resolve().then(() => console.log('then', als.getStore()));
    });
  });
}

async function caseRunReturnAndArgs() {
  console.log('== run-return ==');
  const als = new AsyncLocalStorage<string>();
  console.log('sync-ret', als.run('x', () => 42));
  console.log('args-ret', als.run('y', (a: number, b: number) => a + b, 10, 20));
  console.log('async-ret', await als.run('z', async () => {
    await Promise.resolve();
    return als.getStore();
  }));
  console.log('exit-ret', als.run('w', () => als.exit((s: string) => s + ':' + als.getStore(), 'e')));
}

async function caseConcurrentStores() {
  console.log('== concurrent ==');
  const als = new AsyncLocalStorage<number>();
  const seen: string[] = [];
  async function task(id: number, ms: number) {
    await sleep(ms);
    seen.push('task ' + id + ' sees ' + als.getStore());
    await sleep(ms);
    seen.push('task ' + id + ' still sees ' + als.getStore());
  }
  const p1 = als.run(1, () => task(1, 8));
  const p2 = als.run(2, () => task(2, 3));
  await Promise.all([p1, p2]);
  console.log(seen.sort().join('\n'));
}

async function caseDynamicReceiver() {
  console.log('== dynamic-receiver ==');
  const als: any = new AsyncLocalStorage();
  const r = als.run({ x: 1 }, (a: number, b: number) => {
    console.log('dyn-run', als.getStore().x, a + b);
    return 'ok';
  }, 3, 4);
  console.log('ret', r, als.getStore());

  let resource: AsyncResource | undefined;
  const captured = new AsyncLocalStorage<string>();
  captured.run('captured', () => {
    resource = new AsyncResource('TEST');
  });
  captured.run('current', () => {
    resource!.runInAsyncScope(() => console.log('inScope', captured.getStore()));
    console.log('after', captured.getStore());
  });
}

async function caseSnapshotBind() {
  console.log('== snapshot-bind ==');
  const als = new AsyncLocalStorage<string>();
  let bound: () => string | undefined;
  let snap: any;
  als.run('bind-ctx', () => {
    bound = AsyncLocalStorage.bind(() => als.getStore());
    snap = AsyncLocalStorage.snapshot();
  });
  als.run('other', () => {
    console.log('bound', bound());
    console.log('snap', snap(() => als.getStore()));
    console.log('direct', als.getStore());
  });
}

async function main() {
  await caseBasicAwait();
  await caseNestedRun();
  await caseEnterWithInRun();
  await caseCrossStorage();
  await caseThrowRestores();
  await caseResourceThrow();
  await caseMicrotaskOrdering();
  await caseRunReturnAndArgs();
  await caseConcurrentStores();
  await caseDynamicReceiver();
  await caseSnapshotBind();
  console.log('done');
}

main();
