### Share one CPU-dispatched inflate implementation

One-shot and streaming gunzip/inflate now use the same decoder and zlib-ng state. This removes the separate one-shot decoder path and unsafe zero-initialization of miniz inflate state. The stream and its aligned working memory use the existing payload allocator, with exact accounting and release on close. Compression keeps its existing backend and output.

Chunk-boundary, concatenated-member, checksum, truncation and allocator-failure tests cover the shared decoder. A CPU-qualified instruction witness records the previous backend as a negative control.
