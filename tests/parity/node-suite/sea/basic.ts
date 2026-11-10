import * as sea from "node:sea";
import {
  getAsset,
  getAssetAsBlob,
  getAssetKeys,
  getRawAsset,
  isSea,
} from "node:sea";
import { isBuiltin } from "node:module";

function outcome(fn: () => unknown): string {
  try {
    const value = fn();
    return `ok:${Object.prototype.toString.call(value)}:${String(value)}`;
  } catch (error) {
    const err = error as Error & { code?: string };
    return `err:${err.name}:${err.code}:${err.message}`;
  }
}

const keys = Object.keys(sea).sort();
console.log("keys:", keys.join(","));

for (const key of keys) {
  const fn = (sea as Record<string, Function>)[key];
  console.log(`fn ${key}:`, typeof fn, fn.length, fn.name);
}

console.log("named same:", isSea === sea.isSea, getAsset === sea.getAsset);
console.log(
  "named same 2:",
  getRawAsset === sea.getRawAsset,
  getAssetAsBlob === sea.getAssetAsBlob,
  getAssetKeys === sea.getAssetKeys,
);
console.log("isSea:", sea.isSea(), isSea(), (sea as any).isSea(1));
console.log("module isBuiltin:", isBuiltin("sea"), isBuiltin("node:sea"));

const bareBuiltin = process.getBuiltinModule("sea");
const nodeBuiltin = process.getBuiltinModule("node:sea") as Record<string, unknown>;
console.log("process bare:", String(bareBuiltin));
console.log("process node keys:", Object.keys(nodeBuiltin).join(","));
console.log("process node same:", nodeBuiltin.isSea === sea.isSea);

console.log("asset keys:", outcome(() => sea.getAssetKeys()));
console.log("asset keys extra:", outcome(() => (sea as any).getAssetKeys(1)));
console.log("asset missing:", outcome(() => sea.getAsset("missing")));
console.log("asset missing utf8:", outcome(() => sea.getAsset("missing", "utf8")));
console.log("raw missing:", outcome(() => sea.getRawAsset("missing")));
console.log("blob missing:", outcome(() => sea.getAssetAsBlob("missing")));
console.log("asset no arg:", outcome(() => (sea as any).getAsset()));
console.log("asset number:", outcome(() => (sea as any).getAsset(1)));
console.log("raw no arg:", outcome(() => (sea as any).getRawAsset()));
console.log("blob number:", outcome(() => (sea as any).getAssetAsBlob(1)));
