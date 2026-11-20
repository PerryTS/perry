// Non-entry module half of test_gap_10455_class_expr_factory_identity.ts —
// the exact shape redis's `commander.js` `attachConfig` uses
// (`Class = class extends BaseClass {}`), reached through an ordinary
// cross-module call.
export function withCommands(Base: any) {
  return class extends Base {};
}

export function withCommandsB(Base: any) {
  return class extends Base {};
}
