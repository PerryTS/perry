Fix `ArrayBuffer.isView`, `util.types.isArrayBufferView`, `util.types.isUint8Array`, and `util.types.isTypedArray` returning false for Node Buffers (#11239).

Both direct calls and extracted `ArrayBuffer.isView` calls now recognize Buffer-backed typed arrays through the existing typed-array brand classifier. Raw ArrayBuffer/SharedArrayBuffer, DataView, and crypto key backings retain their distinct brands. Regression coverage includes allocated Buffers, subarrays, shared backing spans, and the view/byte-length check used by bson UUID decoding.
