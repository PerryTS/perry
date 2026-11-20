// jose JWT compile-as-package smoke. Uses the original task spec - native
// createSecretKey now wires through to a Uint8Array-marked BufferHeader
// that the bridge materializes as a real v8::Uint8Array when handed to
// jose inside the V8 fallback. See CHANGELOG entry for the full fix.
import { SignJWT } from 'jose';
import { createSecretKey } from 'node:crypto';
const key = createSecretKey('secret', 'utf8');
const token = await new SignJWT({ a: 1 })
    .setProtectedHeader({ alg: 'HS256' })
    .sign(key);
console.log(typeof token);             // 'string'
console.log(token.split('.').length);  // 3
