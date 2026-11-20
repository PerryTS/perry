// Mirrors ethers' `src.ts/providers/ws.ts`: a one-line facade module that
// re-exports a named binding from a Perry-native npm package which is NOT a
// Node core builtin. See test_gap_11044_native_facade_reexport_construct.ts
// and issue #11044.
export { WebSocket } from "ws";
