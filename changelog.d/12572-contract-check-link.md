Define  in every runtime build (#12572).
Codegen emits calls to it under , but the symbol was
gated on , which contract builds no longer enable, so they
failed to link. The function only reads the existing pristine flags; release
code still never references it.
