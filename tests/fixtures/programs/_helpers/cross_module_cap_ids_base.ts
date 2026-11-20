// Helper module for test_gap_cross_module_class_capture_ids.ts.
//
// Shaped like next/dist/server/base-server.js: a class whose members read
// module-scope locals (class captures), including a virtual method the
// CONSTRUCTOR calls — so the derived override runs while only the BASE
// constructor's `__perry_cap_*` stashes exist on the instance. The captured
// locals here are declared in the same order as the derived module's, so the
// capture LOCAL IDS collide across the two modules (they restart per module).
const baseInterop = { default: "BASE-path-ns" };
const baseExtra = "base-extra";

export class BaseServer {
  tag: string;
  constructor() {
    // Virtual call from the base ctor — lands in the derived override before
    // the derived ctor's own capture stashes run (the Next.js
    // `this.buildId = this.getBuildId()` shape).
    this.tag = this.describe();
  }
  describe(): string {
    return "base:" + baseInterop.default + ":" + baseExtra;
  }
  baseView(): string {
    return baseInterop.default;
  }
}
