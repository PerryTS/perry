// Non-entry module half of test_gap_11042_class_expr_dynamic_heritage_per_evaluation.ts:
// `@redis/client`'s `commander.js` `attachConfig`, minus the module/function
// namespaces — a comma declarator whose second binding is an anonymous class
// expression with dynamic heritage, then prototype-assigned commands.
export function attachConfig({ BaseClass, commands, createCommand }: any) {
  const RESP = 2,
    Class = class extends BaseClass {};
  for (const [name, command] of Object.entries(commands)) {
    Class.prototype[name] = createCommand(command, RESP);
  }
  return Class;
}
