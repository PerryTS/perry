// Regression guard for the PR's `extern_func.rs` change: lowering of
// `node:*` submodule named-import calls now flows through
// `js_closure_callN` and preserves up to 16 args, replacing the
// earlier "discard args, call0" path. We exercise that path against
// `node:diagnostics_channel` thunks of different arities (channel/1,
// subscribe/2, hasSubscribers/1, unsubscribe/2, tracingChannel/1) so
// a future arity-adaptation regression surfaces as a parity break.
//
// All assertions check pure boolean/typeof shape — no impl details
// that could drift between Node and Perry.
import {
  channel,
  subscribe,
  unsubscribe,
  hasSubscribers,
  tracingChannel,
} from "node:diagnostics_channel";

const ch = channel("dc-thunk-arity");
console.log("channel/1:", typeof ch === "object");

const h = () => {};
subscribe("dc-thunk-arity", h);
console.log("subscribe/2 after:", hasSubscribers("dc-thunk-arity"));

console.log("unsubscribe/2:", unsubscribe("dc-thunk-arity", h));
console.log("hasSubscribers/1 after:", hasSubscribers("dc-thunk-arity"));

const trc = tracingChannel("dc-thunk-trace");
console.log("tracingChannel/1:", typeof trc === "object");
