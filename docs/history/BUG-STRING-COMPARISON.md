# Bug: String `!==` comparison fails for concatenated strings

**Severity:** Critical
**Affected:** Perry runtime (compiled TypeScript)
**Found:** 2026-03-25
**Context:** perry-hub token authentication

## Summary

The `!==` (strict inequality) operator returns `true` (not equal) for two strings that are demonstrably identical. This breaks all token-based authentication in perry-hub and likely affects any TypeScript code that compares dynamically constructed strings.

## Reproduction

```typescript
const SECRET = process.env.MY_SECRET || ''; // e.g. "abc123"
const header = request.headers['authorization'] || ''; // e.g. "Bearer abc123"
const expected = 'Bearer ' + SECRET; // "Bearer abc123"

// This SHOULD be false (strings are equal), but perry returns true:
if (header !== expected) {
  // ALWAYS enters this branch, even when the strings match
  console.log('mismatch!');
}
```

## Evidence

Added debug logging to perry-hub that prints both operands before comparison:

```
AUTH_DEBUG: auth="Bearer 62E77FBB-57B8-4611-A361-156288F5C8CF"
           expected="Bearer 62E77FBB-57B8-4611-A361-156288F5C8CF"
```

The strings are byte-for-byte identical (same length, same content), yet `auth !== expected` evaluates to `true`.

## Characteristics

- `!==` fails, `!=` also fails (both return wrong result)
- Affects strings built via concatenation (`'Bearer ' + variable`)
- Affects strings from `process.env` concatenated with literals
- `===` likely also affected (returns `false` for equal strings)
- `endsWith()` and `startsWith()` work correctly as workarounds
- Comparison of two string literals works fine (`'foo' !== 'foo'` is correctly `false`)

## Workaround

Replace:
```typescript
if (auth !== 'Bearer ' + SECRET) { ... }
```

With:
```typescript
if (!auth.endsWith(SECRET) || !auth.startsWith('Bearer ')) { ... }
```

## Impact

This bug broke ALL of these in perry-hub:
1. Admin API authentication (`/api/v1/admin/update-perry`)
2. Tarball download authentication (`/api/v1/tarball/:jobId`)
3. Artifact upload authentication (`/api/v1/artifact/upload/:jobId`)

All build jobs failed with "builder error" or "403 Forbidden" because workers couldn't download tarballs from the hub.

## Likely root cause

The perry compiler's string representation may use different internal types for:
- String literals (compile-time constant)
- `process.env` values (runtime-allocated)
- Concatenation results (`+` operator)

The `!==` / `===` codegen may be comparing pointers or internal type tags instead of string content. The `endsWith`/`startsWith` methods correctly compare character content.

## Files affected by workaround

- `perry-hub/src/main.ts` — 5 occurrences of `!==` replaced with `endsWith`/`startsWith`
