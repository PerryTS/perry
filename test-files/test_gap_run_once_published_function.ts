// A function named once in its module runs once there only when the module
// does not publish it. `cached` is exported: other modules call it with their
// own getters. Per-unit facts flattened its one call in util.ts into module
// scope, proved `getter()` a call of util.ts's own `() => true`, and codegen
// called that body for every importer: `box()` gave `true`, not the object
// (zod 4.4.3: `undefined is not iterable at generateFastpass`). The same for
// an exported arrow, a function stored into an exported object, and the
// CommonJS export.
import { cached, flag } from "./_helpers/run_once_closed/util.ts";
import { named } from "./_helpers/run_once_closed/other.ts";
import { lazy, ready } from "./_helpers/run_once_closed/arrow.ts";
import { api, made } from "./_helpers/run_once_closed/stored.ts";
import twin from "./_helpers/run_once_closed/twin.cjs";

const box = cached(() => ({ keys: ["a", "b"] }));
console.log(typeof box(), JSON.stringify(box()), flag(), named());
const late = lazy(() => ({ keys: ["c"] }));
console.log(typeof late(), JSON.stringify(late()), ready());
const via = api.make(() => ({ keys: ["d"] }));
console.log(typeof via(), JSON.stringify(via()), made());
const cj = twin.cached(() => ({ keys: ["e"] }));
console.log(typeof cj(), JSON.stringify(cj()), twin.flag());
