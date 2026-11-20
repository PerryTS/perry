import { AsyncLocalStorage, AsyncResource } from "node:async_hooks";

class Implicit extends AsyncLocalStorage {}
class Explicit extends AsyncLocalStorage {
  constructor() {
    super();
  }
}

console.log("implicit native parent", Object.getPrototypeOf(Implicit) === AsyncLocalStorage);
console.log("explicit native parent", Object.getPrototypeOf(Explicit) === AsyncLocalStorage);

function makeStorageClass() {
  return class extends AsyncLocalStorage {};
}

const first = makeStorageClass();
const second = makeStorageClass();
console.log("fresh constructors", first !== second);
console.log("first native parent", Object.getPrototypeOf(first) === AsyncLocalStorage);
console.log("second native parent", Object.getPrototypeOf(second) === AsyncLocalStorage);
Object.setPrototypeOf(first, AsyncResource);
console.log("retarget first", Object.getPrototypeOf(first) === AsyncResource);
console.log("preserve second", Object.getPrototypeOf(second) === AsyncLocalStorage);
Object.setPrototypeOf(first, null);
const third = makeStorageClass();
console.log("preserve null", Object.getPrototypeOf(first) === null);
console.log("preserve second again", Object.getPrototypeOf(second) === AsyncLocalStorage);
console.log("third native parent", Object.getPrototypeOf(third) === AsyncLocalStorage);
