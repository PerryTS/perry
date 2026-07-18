// Regression: cross-module base/derived class-capture field collision.
//
// Class captures are stashed on the instance as `this.__perry_cap_<id>`
// fields by each class's constructor. Local ids restart per module, and
// `super()` runs the PARENT constructor's stashes on the SHARED instance —
// so with bare per-id names, a derived method rebinding its own capture id K
// read the PARENT MODULE's captured local K whenever the ids collided.
//
// This is exactly how Next.js standalone died at boot: base-server.js
// captures `_path = _interop_require_wildcard(require("path"))` and
// next-server.js captures `_fs = _interop_require_default(require("fs"))`
// under the same local id; `Server`'s ctor calls the virtual
// `this.getBuildId()`, whose `_fs.default.readFileSync(...)` rebind found
// base-server's `_path` wildcard instead — `readFileSync` dispatched on the
// `path` namespace, returned undefined, and `.trim()` threw before Ready.
//
// The fix salts the field names per defining module
// (`__perry_cap_<id>m<salt>`, crates/perry-hir/src/cap_fields.rs), keeping
// same-module inheritance sharing intact.
//
// Output is byte-identical to `node --experimental-strip-types`.

import { BaseServer } from "./_helpers/cross_module_cap_ids_base.ts";

// Same declaration order as the helper module → colliding capture local ids.
const derivedInterop = { default: "DERIVED-fs-ns" };
const derivedExtra = "derived-extra";

class DerivedServer extends BaseServer {
  constructor() {
    super();
  }
  // Virtual override the BASE ctor calls before this module's stashes exist.
  describe(): string {
    return "derived:" + derivedInterop.default + ":" + derivedExtra;
  }
}

const d = new DerivedServer();
// Pre-fix: tag read base-server's captures ("derived:BASE-path-ns:base-extra")
// or undefined-flavored garbage. Post-fix: the derived override sees its own.
console.log("tag:", d.tag);
// After construction both stash sets exist — reads must stay per-class.
console.log("post:", d.describe(), "|", d.baseView());
// A base instance is unaffected either way.
console.log("base:", new BaseServer().tag);
