# Utilities

Perry natively implements common utility packages.

## lodash

The `lodash` runtime functions are partially implemented (see
`crates/perry-stdlib/src/lodash.rs`) but the user-facing dispatch from
`import _ from "lodash"; _.chunk(...)` is not wired into the LLVM backend
yet. Track the follow-up at issue #200.

```text
import _ from "lodash";

_.chunk([1, 2, 3, 4, 5], 2);     // [[1,2], [3,4], [5]]
_.uniq([1, 2, 2, 3, 3]);          // [1, 2, 3]
_.groupBy(users, "role");
_.sortBy(users, ["name"]);
_.cloneDeep(obj);
_.merge(defaults, overrides);
_.debounce(fn, 300);
_.throttle(fn, 100);
```

## dayjs

`dayjs` runtime functions are declared (`js_dayjs_now`, `js_dayjs_format`,
`js_dayjs_add`, etc.) but the user-facing dispatch from
`import dayjs from "dayjs"; dayjs()` chained methods is not wired into the
LLVM backend yet. Track the follow-up at issue #200.

```text
import dayjs from "dayjs";

const now = dayjs();
console.log(now.format("YYYY-MM-DD"));
console.log(now.add(7, "day").format("YYYY-MM-DD"));
console.log(now.subtract(1, "month").toISOString());

const diff = dayjs("2025-12-31").diff(now, "day");
console.log(`${diff} days until end of year`);
```

## moment

Same status as `dayjs` — the runtime functions exist but the dispatch path
is not wired yet.

```text
import moment from "moment";

const now = moment();
console.log(now.format("MMMM Do YYYY"));
console.log(now.fromNow());
console.log(moment("2025-01-01").isBefore(now));
```

## uuid

```typescript
{{#include ../../examples/stdlib/utilities/snippets.ts:uuid}}
```

## nanoid

The default-length `nanoid()` call is wired. The custom-length form
`nanoid(10)` has a runtime function (`js_nanoid_sized`) but no dispatch
yet — track at issue #200.

```typescript
{{#include ../../examples/stdlib/utilities/snippets.ts:nanoid}}
```

## validator

```typescript
{{#include ../../examples/stdlib/utilities/snippets.ts:validator}}
```

## Next Steps

- [Other Modules](other.md)
- [Overview](overview.md) — All stdlib modules
