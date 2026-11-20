import * as vm from "node:vm";

function descriptor(target: object, key: string) {
  let owner: any = target;
  let depth = 0;
  while (owner && !Object.prototype.hasOwnProperty.call(owner, key)) {
    owner = Object.getPrototypeOf(owner);
    depth++;
  }
  const value = owner && Object.getOwnPropertyDescriptor(owner, key);
  console.log(
    key + ":",
    depth,
    value?.enumerable,
    value?.configurable,
    value?.writable,
    typeof value?.value,
  );
}

descriptor(vm, "Script");
descriptor(vm, "createContext");
descriptor(vm, "constants");
const script = new vm.Script("");
descriptor(script, "runInContext");
descriptor(script, "createCachedData");
console.log(
  "prototype constructor:",
  vm.Script.prototype.constructor === vm.Script,
);
