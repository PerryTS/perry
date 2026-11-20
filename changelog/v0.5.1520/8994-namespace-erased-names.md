Module namespaces omit erased and non-consumer-visible names.

Namespace keys are derived from consumer-visible export names, so a private backing function such as `_null` no longer appears beside its public alias, and erased TypeScript interfaces and type aliases are excluded when materializing dynamic and nested namespace objects. Includes a Zod-shaped regression for `import * as z`.
