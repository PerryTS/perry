// Ordinary shaped descriptors must protect bag metadata without a name guard.
for (const bytes of [new Uint8Array(4), new Uint32Array(4), Buffer.alloc(4)]) {
  bytes[0] = 19;
  Object.defineProperty(bytes, 'edge', {
    value: 17, writable: false, configurable: false, enumerable: false,
  });
  Object.defineProperty(bytes, 'edge', { value: 17 });
  let rejected = 0;
  try { Object.defineProperty(bytes, 'edge', { value: 18 }); }
  catch (error) { if (error instanceof TypeError) rejected++; }
  try { Object.defineProperty(bytes, 'edge', { enumerable: true }); }
  catch (error) { if (error instanceof TypeError) rejected++; }
  console.log((bytes as any).edge, rejected, Reflect.deleteProperty(bytes, 'edge'), bytes[0]);
}
