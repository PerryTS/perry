# node:events granular parity suite

Small, deterministic TypeScript parity cases for `node:events`, curated from the current smoke inventory and Node/Deno EventEmitter API behavior.

Intentional scope: EventEmitter listener tables, once/on helpers, errors, max listeners, symbols, and module import shapes. Avoid async-resource coverage because async IDs are non-deterministic.
