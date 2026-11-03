A program with a dynamic `import()` no longer gets a
`globalThis.AsyncLocalStorage` that node does not have. The entry seeds that
global only for Next.js programs, and every module loaded by `import()` used
to count as a Next.js module. Such a module now gets its path initializer
registered lazily, like an on-demand CommonJS module, so startup also skips
one registry insert per deferred module.
