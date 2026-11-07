// The sources are runtime values: a literal eval/new Function source can be
// compiled ahead of time and would never reach the runtime parser.
const parts = [String(1), "+", String(2)];
const body = ["return", String(3)].join(" ");
export const v = [eval(parts.join(" ")), new Function(body)()];
