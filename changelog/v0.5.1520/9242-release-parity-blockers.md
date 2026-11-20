Method calls now preserve same-named instance fields declared on the receiver
class or an ancestor instead of bypassing them with direct prototype dispatch.
The default-runtime WebAssembly parity fixture also reliably stays out of the
auto-linked host mode, restoring coverage of graceful degradation.
