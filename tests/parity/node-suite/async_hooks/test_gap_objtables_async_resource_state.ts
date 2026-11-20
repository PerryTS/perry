import { AsyncLocalStorage, AsyncResource, createHook, executionAsyncId, executionAsyncResource } from 'node:async_hooks';
const storage = new AsyncLocalStorage<{value: number}>();
let resource: AsyncResource;
let destroys = 0;
let serial = 0;
const hook = createHook({
  init(id, type) { if (type === 'objtables-state') serial = id; },
  destroy(id) { if (id === serial) destroys++; }
}).enable();
storage.run({value: 73}, () => { resource = new AsyncResource('objtables-state'); });
const pressure: unknown[] = [];
for (let i = 0; i < 40000; i++) pressure.push({value: i, other: {value: i}});
resource.runInAsyncScope(() => {
  console.log('state', storage.getStore()?.value, executionAsyncId() === resource.asyncId(), executionAsyncResource() === resource);
});
console.log('public-id', resource.asyncId() === serial, serial > 1, Object.keys(resource).length);
resource.emitDestroy();
resource.emitDestroy();
resource.runInAsyncScope(() => console.log('reusable', storage.getStore()?.value));
setImmediate(() => {
  console.log('destroy', destroys);
  storage.disable();
  const disabledResource = new AsyncResource('objtables-disabled');
  disabledResource.runInAsyncScope(() => console.log('disabled', storage.getStore() === undefined));
  hook.disable();
});
