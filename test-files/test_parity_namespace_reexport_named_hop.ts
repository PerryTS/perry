// A namespace binding must keep its kind through an `import { X } from "m";
// export { X }` hop, the one re-export form the namespace scan did not follow
// (#11275 covered `export { X } from "m"` and `export *`).
//
// Unfollowed, the local was never classified as a namespace: `NS.translate`
// lowered to a closure singleton over `__perry_wrap_<ns module>__NS$info` plus
// a dynamic method lookup, and that wrapper does not exist for a namespace —
// the link failed with `undefined reference`. Reached from trails through
// activesupport's `import * as I18n from "@blazetrails/i18n"; export { I18n }`.
import { NS } from "./fixtures/namespace_reexport_named_hop/mid.ts";

console.log(NS.translate("x"));
console.log(typeof NS.t);
