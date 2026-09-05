Fix the native-root provider gate's missing ReadableStream and Response helpers,
and prevent separately built runtime providers from assigning different TLS
values to the same shared cache slot. Provider copies of a thread-local now
claim one declaration identity and reuse its existing storage, preserving the
class registry and GC state while streamed Responses run under moving GC.
