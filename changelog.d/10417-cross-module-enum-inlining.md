Cross-module inlining now keeps functions and methods that reference TypeScript
enum members in their source module. Previously, copying such a body into an
importer made code generation resolve the enum against the importer's enum
table, causing compilation to fail with `enum member X.Y not found in enums
table`. This unblocks packages such as cheerio that use small exported helpers
backed by module-local enums (#10417).
