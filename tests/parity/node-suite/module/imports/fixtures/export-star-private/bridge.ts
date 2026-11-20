import * as core from "./barrel.ts";

export function coreNamespace(): typeof core {
  return core;
}

export { foo } from "./barrel.ts";
