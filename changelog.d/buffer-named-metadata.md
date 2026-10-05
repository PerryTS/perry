Read Buffer and typed-array `length`, `byteLength`, and `byteOffset` through a guarded native metadata path. Source `.length` reads admit byte views and shared typed storage and pass a pooled, GC-rooted literal key to the cold path. Dynamic known-name reads avoid ordinary object shape lookup, UTF-8 conversion, and key allocation.

Withdraw the default-accessor proof before own metadata definitions, accessor/prototype edits, and custom view prototype links. The fallback preserves the actual getter receiver and roots inputs across user code. Read live lengths and existing offset metadata after resize/detach; leave native arena views on their validating path. No new receiver registry or heap-pointer cache is added.

Add Node parity coverage for own and inherited metadata, custom/null prototypes, Reflect.construct, offset views, resizable/detached buffers, and a collecting accessor; add runtime and IR tests for positive layout proof, invalidation, and pooled keys.

Keep the intrinsic Buffer prototype in a traced native-constructor capture and preserve its original Uint8Array parent in the existing intrinsic constructor capture. Wire the parent lazily during ordinary prototype reads. Reassigning the writable `Buffer.prototype` property cannot retarget existing or freshly allocated Buffer instances, and edits through an alias of the original prototype still withdraw the metadata proof. The constructor builder is extracted into a small module to retain the repository's file-size limit.

Correct detached typed-array byteOffset to zero through the shared offset helper, including fixed and length-tracking offset views.
