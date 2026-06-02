// Issue #818 follow-up: verify that a V8-fallback binary embeds the
// imported JS module sources at compile time and can run without the
// project's node_modules/ at runtime.
//
// Build, then move the resulting executable to a directory that does NOT
// contain node_modules/. Both prints should still succeed.
//
// Expected output:
//   object
//   function
//   self-contained ok

import { Hono } from 'hono';

const app = new Hono();
app.get('/', (c) => c.text('Hi'));

console.log(typeof app);
console.log(typeof app.get);
console.log('self-contained ok');
