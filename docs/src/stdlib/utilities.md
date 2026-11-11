# Utilities

Perry natively implements common utility packages.

## lodash

`lodash` has no native shim: `import _ from "lodash"` (and named or
per-method imports such as `lodash/chunk`) compiles the real npm package from
source, so the whole lodash API is available, not a subset.

```typescript,no-test
import _ from "lodash";

_.chunk([1, 2, 3, 4, 5], 2); // [[1,2], [3,4], [5]]
_.uniq([1, 2, 2, 3, 3]);      // [1, 2, 3]
_.range(0, 5, 1);              // [0, 1, 2, 3, 4]
_.sum([2, 3, 4]);              // 9
_.camelCase("hello world");    // "helloWorld"
```

`_.template` with its default options builds its render function with a
`with` statement, which Perry's runtime `Function` interpreter does not run
yet (#13962). Pass `{ variable: "data" }` to avoid it.

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
