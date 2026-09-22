// #10453: `class X extends AsyncResource` threw "Class constructor
// AsyncResource cannot be invoked without 'new'" at `super()` for every
// heritage shape EXCEPT a bare `import { AsyncResource } from
// "node:async_hooks"` binding. A local alias (`const Alias = AsyncResource`),
// a namespace member (`ah.AsyncResource`), and a CJS destructured
// `require('node:async_hooks')` all reach the same bound native export
// VALUE the bare import does, but HIR lowering only recognized the bare
// import shape statically, so `super()` for the other shapes fell through to
// a plain CALL of the native export — and `AsyncResource` throws by design
// when invoked without `new`.
//
// `undici` (`lib/api/api-request.js` etc.) uses exactly the CJS destructured
// shape: `const { AsyncResource } = require('node:async_hooks'); class …
// extends AsyncResource`.
import { AsyncResource } from "node:async_hooks";
import * as ah from "node:async_hooks";
import { Plain, InTry, ViaMemberExport } from "./gap_10453_asyncresource_heritage_helper.cjs";

const Alias = AsyncResource;

class ViaImport extends AsyncResource {
  constructor() {
    super("X");
  }
}
class ViaAlias extends Alias {
  constructor() {
    super("X");
  }
}
class ViaNamespace extends ah.AsyncResource {
  constructor() {
    super("X");
  }
}
function t(name: string, C: any, ...args: unknown[]) {
  try {
    const r = new C(...args);
    console.log(
      name,
      "ok",
      typeof r.runInAsyncScope,
      typeof r.triggerAsyncId(),
      r instanceof AsyncResource,
    );
  } catch (e: any) {
    console.log(name, "threw:", e.message);
  }
}

t("TS  extends AsyncResource (import)  ", ViaImport, "X");
t("TS  extends Alias                   ", ViaAlias, "X");
t("TS  extends ah.AsyncResource        ", ViaNamespace, "X");
t("CJS destructured require, super()   ", Plain, "X");
t("CJS destructured require, try{super}", InTry, "X");
t("CJS namespace member export         ", ViaMemberExport, "X");
