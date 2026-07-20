# Interpolation & Plurals

## Parameterized Strings

Use `{param}` placeholders in your strings and pass values as a second argument:

```typescript
{{#include ../../examples/i18n/snippets.ts:interp-params}}
```

Translation files use the same `{param}` syntax:

```json
// locales/en.json
{
  "Hello, {name}!": "Hello, {name}!",
  "Total: {price}": "Total: {price}"
}

// locales/de.json
{
  "Hello, {name}!": "Hallo, {name}!",
  "Total: {price}": "Gesamt: {price}"
}
```

Parameters are substituted at runtime after the locale-appropriate template is selected. The substitution handles any value type (numbers, strings, dates) by converting to string.

### Diagnostics & Fallbacks

Perry reports translation-table problems as build warnings and falls back safely at runtime:

| Condition | Behavior |
|-----------|----------|
| Key missing from a locale file | Build **warning**; that locale renders the default locale's text |
| Key present in a locale file but never used in code | Build **warning** ("unused i18n key") |
| Translation cell left empty (`""`, the "needs translation" marker) | Renders the source key's text |
| `{param}` in the template but not passed at the call site | Renders the literal `{param}` text so the mismatch is visible |
| Param passed at the call site but not referenced by the template | Value is evaluated (side effects run in source order) and ignored |

## Plural Rules

Plural forms use dot-suffix keys based on CLDR plural categories: `.zero`, `.one`, `.two`, `.few`, `.many`, `.other`.

### Locale Files

```json
// locales/en.json
{
  "You have {count} items.one": "You have {count} item.",
  "You have {count} items.other": "You have {count} items."
}

// locales/de.json
{
  "You have {count} items.one": "Du hast {count} Artikel.",
  "You have {count} items.other": "Du hast {count} Artikel."
}

// locales/pl.json (Polish: one, few, many)
{
  "You have {count} items.one": "Masz {count} element.",
  "You have {count} items.few": "Masz {count} elementy.",
  "You have {count} items.many": "Masz {count} elementow.",
  "You have {count} items.other": "Masz {count} elementu."
}
```

### Usage in Code

Reference the base key without any suffix. Perry detects the plural variants automatically:

```typescript
{{#include ../../examples/i18n/snippets.ts:interp-plural}}
```

Perry detects the plural parameter from the first `{param}` placeholder in the key (here `{count}`). At runtime, each call site evaluates the current locale's CLDR rules against the passed value and selects the matching form's translation — a compiled binary run with `LANG=de_DE.UTF-8` picks the German `.one` form for `count: 1` and the German `.other` form for `count: 3`. A call site that doesn't pass the plural parameter renders the base (non-plural) key.

### Supported Locales

Perry includes hand-rolled CLDR plural rules for 30+ locales:

| Pattern | Locales |
|---------|---------|
| one/other | English, German, Dutch, Swedish, Danish, Norwegian, Finnish, Estonian, Hungarian, Turkish, Greek, Hebrew, Italian, Spanish, Portuguese, Catalan, Bulgarian, Hindi, Bengali, Swahili, ... |
| one (0-1) / other | French |
| no distinction | Japanese, Chinese, Korean, Vietnamese, Thai, Indonesian, Malay |
| one/few/many | Russian, Ukrainian, Serbian, Croatian, Bosnian, Polish |
| one/few/other | Czech, Slovak |
| zero/one/two/few/many/other | Arabic |
| one/few/other | Romanian, Lithuanian |
| zero/one/other | Latvian |

### Fallback Order

When the selected CLDR category has no corresponding form:

| Situation | Behavior |
|-----------|----------|
| Category matches a defined form (e.g. `.one`) | That form's translation, in the active locale |
| Category has no defined form, `.other` exists | The `.other` form |
| Category has no defined form, no `.other` | The base key's translation |
| A locale file is missing a form key | Build **warning**; that locale renders the default locale's form |

## Explicit API for Non-UI Strings

For strings outside UI components (API responses, notifications, etc.), use `t()`:

```typescript
{{#include ../../examples/i18n/snippets.ts:interp-explicit-t}}
```

This uses the same key lookup, locale selection, and interpolation as UI strings.
