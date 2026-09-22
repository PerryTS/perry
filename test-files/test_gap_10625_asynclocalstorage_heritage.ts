// #10625: `class X extends AsyncLocalStorage` has the same indirect-heritage
// defect #10621 fixed for AsyncResource (#10453) — every heritage shape
// EXCEPT a bare `import { AsyncLocalStorage } from "node:async_hooks"`
// binding fell through `js_fetch_or_value_super`
// (`crates/perry-runtime/src/object/global_this/fetch_globals.rs`) to a
// plain CALL of the bound native export, which throws "Class constructor
// AsyncLocalStorage cannot be invoked without 'new'" (or silently produces a
// class_id=0 instance whose inherited methods are missing, depending on the
// shape) instead of running the native-backing init the canonical import
// path already used.
//
// AsyncLocalStorage's subclass-init helper (`js_async_local_storage_subclass_init`)
// lives in perry-stdlib, not perry-runtime, so — unlike AsyncResource, whose
// implementation is entirely in perry-runtime — the fix routes through a
// registration hook perry-stdlib installs at startup
// (`JS_NATIVE_ASYNC_LOCAL_STORAGE_SUBCLASS_INIT`), since perry-runtime
// cannot depend on perry-stdlib.
import { AsyncLocalStorage } from "node:async_hooks";
import * as ah from "node:async_hooks";
import ahDefault from "node:async_hooks";
import {
  ViaRequire,
  ViaRequireNamespaceMember,
} from "./gap_10625_asynclocalstorage_heritage_helper.cjs";

const Alias = AsyncLocalStorage;

class ViaImport extends AsyncLocalStorage {
  constructor() {
    super();
  }
}
class ViaAlias extends Alias {
  constructor() {
    super();
  }
}
class ViaNamespace extends ah.AsyncLocalStorage {
  constructor() {
    super();
  }
}
class ViaDefaultImport extends ahDefault.AsyncLocalStorage {
  constructor() {
    super();
  }
}

function t(name: string, C: any) {
  try {
    const inst = new C();
    // Round-trip through run()/getStore(), not just construction: a
    // class_id=0 empty-object subclass instance would also survive `new`
    // without throwing, so proving the fix needs the store to actually flow.
    const outside = inst.getStore();
    const inside = inst.run(42, () => inst.getStore());
    const nested = inst.run("outer", () =>
      inst.run("inner", () => inst.getStore()),
    );
    console.log(
      name,
      "ok",
      "outside=" + String(outside),
      "inside=" + inside,
      "nested=" + nested,
      inst instanceof AsyncLocalStorage,
      typeof inst.run,
      typeof inst.getStore,
      typeof inst.enterWith,
      typeof inst.exit,
      typeof inst.disable,
    );
  } catch (e: any) {
    console.log(name, "threw:", e.message);
  }
}

t("TS  extends AsyncLocalStorage (import)   ", ViaImport);
t("TS  extends Alias                        ", ViaAlias);
t("TS  extends ah.AsyncLocalStorage         ", ViaNamespace);
t("TS  extends default.AsyncLocalStorage    ", ViaDefaultImport);
t("CJS destructured require                 ", ViaRequire);
t("CJS namespace member export              ", ViaRequireNamespaceMember);
