// #9778: export lists refer to bindings, irrespective of initializer shape
// or declaration order. Keep this fixture independent of bundled applications.
export { commands as COMMANDS, lookup as LOOKUP, total as TOTAL, empty as EMPTY };
var commands = new Set(['doctor']);
let lookup = new Map([['doctor', true]]);
const total = 40 + 2;
let empty;
function check(args) { return args.includes('doctor'); }
export { check as checkArgs };
