// Scheduler queue entries own their async-resource state: a nextTick,
// queueMicrotask, setTimeout or setImmediate job that is still pending across
// a FULL collection keeps its async id, trigger id, resource and
// AsyncLocalStorage store. A job that already ran gives its state up, and new
// jobs scheduled after a collection must not alias a pending job's state.
// Phase 1 runs with no hook enabled, phase 2 with init/before/after/destroy.
import { AsyncLocalStorage, createHook, executionAsyncId, executionAsyncResource, triggerAsyncId } from 'node:async_hooks';

declare function gc(): void;
function collect(): void {
  if (typeof gc === 'function') gc();
}

const KINDS: Record<string, string> = { tick: 'TickObject', micro: 'Microtask', timeout: 'Timeout', immediate: 'Immediate' };
const storage = new AsyncLocalStorage<{ value: number }>();
const inits = new Map<number, { type: string; trigger: number; resource: object }>();
const befores: number[] = [];
const afters: number[] = [];
const destroys: number[] = [];
let collectInBefore = 0;
const hook = createHook({
  init(id, type, trigger, resource) {
    if (Object.values(KINDS).includes(type)) inits.set(id, { type, trigger, resource });
  },
  before(id) {
    if (!inits.has(id)) return;
    befores.push(id);
    // A collection inside a hook, while the job is popped but still running.
    if (collectInBefore > 0) {
      collectInBefore--;
      collect();
    }
  },
  after(id) {
    if (inits.has(id)) afters.push(id);
  },
  destroy(id) {
    if (inits.has(id)) destroys.push(id);
  },
});

function churn(n: number): unknown[] {
  const out: unknown[] = [];
  for (let i = 0; i < n; i++) out.push({ i, nested: { i, s: 'x' + i } });
  return out;
}

type Seen = { id: number; trigger: number; store: number | undefined; resource: unknown; stable: boolean };
const seen = new Map<string, Seen>();

function job(label: string) {
  return () => {
    const resource = executionAsyncResource();
    churn(200);
    collectInBefore > 0 && collect();
    seen.set(label, {
      id: executionAsyncId(),
      trigger: triggerAsyncId(),
      store: storage.getStore()?.value,
      resource,
      stable: executionAsyncResource() === resource,
    });
  };
}

function schedule(phase: string, from: number, to: number, scheduled: string[]): void {
  for (let i = from; i < to; i++) {
    storage.run({ value: 1000 + i }, () => {
      process.nextTick(job(`${phase}:tick:${i}`));
      queueMicrotask(job(`${phase}:micro:${i}`));
      setTimeout(job(`${phase}:timeout:${i}`), 0);
      setImmediate(job(`${phase}:immediate:${i}`));
    });
    for (const kind of Object.keys(KINDS)) scheduled.push(`${phase}:${kind}:${i}`);
    if (i % 16 === 15) {
      churn(5000);
      collect();
    }
  }
}

function report(phase: string, scheduled: string[], outer: number, hooked: boolean): void {
  let ran = 0, storeOk = 0, stable = 0, triggerOk = 0, resourceObj = 0;
  let initOk = 0, typeOk = 0, resourceOk = 0, lifecycleOk = 0;
  const ids = new Set<number>();
  const resources = new Set<unknown>();
  for (const label of scheduled) {
    const got = seen.get(label);
    if (!got) continue;
    const [, kind, i] = label.split(':');
    ran++;
    ids.add(got.id);
    resources.add(got.resource);
    if (got.store === 1000 + Number(i)) storeOk++;
    if (got.stable) stable++;
    if (got.trigger === outer) triggerOk++;
    if (typeof got.resource === 'object' && got.resource !== null) resourceObj++;
    if (!hooked) continue;
    const init = inits.get(got.id);
    if (!init) continue;
    initOk++;
    if (init.type === KINDS[kind]) typeOk++;
    if (init.resource === got.resource) resourceOk++;
    if (befores.includes(got.id) && afters.includes(got.id) && destroys.includes(got.id)) lifecycleOk++;
  }
  console.log(phase, 'scheduled', scheduled.length, 'ran', ran, 'distinct-ids', ids.size, 'outer-id-reused', ids.has(outer));
  console.log(phase, 'store', storeOk, 'trigger', triggerOk, 'resource-object', resourceObj, 'distinct-resources', resources.size, 'stable', stable);
  if (hooked) console.log(phase, 'init', initOk, 'type', typeOk, 'init-resource', resourceOk, 'before-after-destroy', lifecycleOk);
}

// Phase 1: no hook. Pending across collections, then more jobs after them.
const plain: string[] = [];
const outer = executionAsyncId();
schedule('plain', 0, 48, plain);
collect();
churn(20000);
collect();
schedule('plain', 48, 64, plain);

// Report from a check-phase job queued after every scheduled one, so a slow
// run (forced collections) still sees each timer and immediate run first.
setTimeout(() => setImmediate(() => {
  collect();
  report('plain', plain, outer, false);
  // Phase 2: hooks enabled; collections also run inside before hooks.
  hook.enable();
  const hooked: string[] = [];
  const hookedOuter = executionAsyncId();
  schedule('hooked', 0, 48, hooked);
  collect();
  churn(20000);
  collect();
  schedule('hooked', 48, 64, hooked);
  collectInBefore = 16;
  setTimeout(() => {
    // Node delivers the destroy hooks of timers and immediates
    // asynchronously; report once a later timer turn has seen them.
    setImmediate(() => setTimeout(() => {
      collect();
      report('hooked', hooked, hookedOuter, true);
      hook.disable();
    }, 5));
  }, 5);
}), 5);
