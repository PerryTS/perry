The WebSocket cross-function dispatch regression suite now compiles its
IR-inspection cases with linking disabled, so those checks no longer rebuild
or depend on runtime and extension archives. The one case that verifies
same-named nested handlers at runtime still links and runs with its required
well-known modules forced on.
