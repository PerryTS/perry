Added regression coverage for Node builtin named imports used from natively
compiled dependencies. The exact `@hono/node-server` fallback from
`options.createServer` to its module-scope `http.createServer` import now has an
offline compiler fixture and a real-package listen/fetch/close release smoke,
covering both `http` and `node:http` spellings without relying on app-level
imports.
