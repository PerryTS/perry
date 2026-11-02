`Buffer.isBuffer()` now returns `false` for a plain `Uint8Array`, matching
Node. Perry stores both values in the same `BufferHeader` layout and buffer
registry, so registry membership alone could not distinguish their brands.

The public predicate now also consults the existing constructor-created
`Uint8Array` discriminator. Native APIs keep using the broader storage
predicate where Node accepts both `Buffer` and `Uint8Array` inputs. Regression
coverage exercises direct calls and `Buffer.isBuffer` used as a first-class
function.
