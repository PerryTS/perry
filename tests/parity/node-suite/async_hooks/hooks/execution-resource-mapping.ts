import {
  AsyncResource,
  createHook,
  executionAsyncResource,
} from "node:async_hooks";

const resources = new Map<number, object>();
let targetId = -1;
let beforeMatches = false;
let callbackMatches = false;
let afterMatches = false;
const hook = createHook({
  init(asyncId, _type, _triggerAsyncId, resource) {
    resources.set(asyncId, resource);
  },
  before(asyncId) {
    if (asyncId === targetId) {
      beforeMatches = executionAsyncResource() === resources.get(asyncId);
    }
  },
  after(asyncId) {
    if (asyncId === targetId) {
      afterMatches = executionAsyncResource() === resources.get(asyncId);
    }
  },
}).enable();

const resource = new AsyncResource("MappingResource");
targetId = resource.asyncId();
const initMatches = resources.get(targetId) === resource;
resource.runInAsyncScope(() => {
  callbackMatches = executionAsyncResource() === resource;
});

console.log(
  "execution resource mapping:",
  initMatches,
  beforeMatches,
  callbackMatches,
  afterMatches,
);
resource.emitDestroy();
hook.disable();
