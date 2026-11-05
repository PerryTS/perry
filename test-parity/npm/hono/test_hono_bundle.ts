// Issue #818: hono's `dist/index.js` re-exports from `./hono.js`, which
// re-exports from `./hono-base.js`, etc. — the V8-fallback bundle used
// to include only `index.js`, dropping ~20 transitive ESM submodules.
// This test exists to be run against a project that has `hono` in its
// node_modules; the parity / smoke suite doesn't install npm packages,
// so by itself the test only proves the compiler doesn't choke on the
// `import { Hono } from 'hono';` line. The real bundle-walks-recursively
// validation lives in /tmp/perry-hono in the PR notes.
import { Hono } from 'hono';
const app = new Hono();
app.get('/', (c) => c.text('Hi'));
console.log(typeof app);
console.log(typeof app.get);
console.log(typeof app.fetch);
