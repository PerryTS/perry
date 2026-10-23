Apply preset dictionary bytes in zlib-wrapped DEFLATE sync and streaming APIs,
including Buffer, TypedArray, DataView and ArrayBuffer sources with their view
bounds preserved. Inflaters install the selected dictionary on `Z_NEED_DICT`
and distinguish "Missing dictionary" from "Bad dictionary".

Streams retain owned dictionary bytes across reset without retaining borrowed
JavaScript pointers. Native codec allocations remain in the payload ledger and
dictionary capacity is included in external-memory accounting. Dictionary-bearing
encoders use the already-linked zlib-ng backend; ordinary compression keeps its
existing backend and output.
