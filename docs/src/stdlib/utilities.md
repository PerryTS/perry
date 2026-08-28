# Utilities

Perry natively implements common utility packages.

## lodash

Perry wires a focused lodash subset through **named imports**. Supported
operations include `chunk`, `compact`, `drop`, `first`/`head`, `last`,
`flatten`, `uniq`, `reverse`, `take`, `camelCase`, `kebabCase`, `snakeCase`,
`clamp`, `range`, `times`, `size`, `tail`, `sum`, `mean`, `sumBy`, `meanBy`,
`max`, `min`, `maxBy`, `minBy`, and `inRange`.

Use named imports: the default-import receiver form (`import _ from "lodash";
_.chunk(...)`) is not routed to these native signatures because it would pass
the module object as an extra receiver argument.

```typescript,no-test
import { chunk, uniq, range, sum, camelCase } from "lodash";

chunk([1, 2, 3, 4, 5], 2); // [[1,2], [3,4], [5]]
uniq([1, 2, 2, 3, 3]);      // [1, 2, 3]
range(0, 5, 1);              // [0, 1, 2, 3, 4]
sum([2, 3, 4]);              // 9
camelCase("hello world");    // "helloWorld"
```

## dayjs

The default and named `dayjs` factories and their native instance-method
dispatch are wired. The supported surface includes formatting, component
getters, `valueOf`, `unix`, `toISOString`, `add`, `subtract`, `startOf`,
`endOf`, comparisons, validation, and `diff`.

```typescript,no-test
import dayjs from "dayjs";

const now = dayjs();
console.log(now.format("YYYY-MM-DD"));
console.log(now.add(7, "day").format("YYYY-MM-DD"));
console.log(now.subtract(1, "month").toISOString());

const diff = dayjs("2025-12-31").diff(now, "day");
console.log(`${diff} days until end of year`);
```

## moment

`moment` uses the same native handle model as `dayjs`. Its factory and instance
methods are wired, including formatting, date component getters, arithmetic,
comparisons, `diff`, `clone`, `fromNow`, and `toDate`.

```typescript,no-test
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

Both `nanoid()` and `nanoid(length)` route through the native sized entry point;
an omitted length uses nanoid's 21-character default.

```typescript
{{#include ../../examples/stdlib/utilities/snippets.ts:nanoid}}
```

## slugify

Both the single-argument form and the replacement/options overload route to
the native implementation. The supported options are `replacement`, `lower`,
`strict`, and `trim`; `remove`, `locale`, `extend`, and the complete upstream
character map remain outside the current faithfulness boundary.

```typescript,no-test
import slugify from "slugify";

slugify("Hello World!", { lower: true }); // "hello-world"
slugify("foo bar", "_");                 // "foo_bar"
```

```typescript
{{#include ../../examples/stdlib/utilities/snippets.ts:slugify}}
```

## validator

```typescript
{{#include ../../examples/stdlib/utilities/snippets.ts:validator}}
```

## Next Steps

- [Other Modules](other.md)
- [Overview](overview.md) — All stdlib modules
