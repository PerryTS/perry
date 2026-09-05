### Fixes
- Lower direct and computed-literal `import.meta.require()` calls through synchronous compiled-module dispatch. Relative and Bun virtual chunk paths are discovered ahead of time, return their namespace immediately, and initialize once when loaded. Fixes #9742.
