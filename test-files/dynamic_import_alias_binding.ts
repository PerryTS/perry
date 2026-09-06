var aliasedVar = new Set(["var-before"]);
let aliasedLet = new Set(["let-before"]);
const aliasedConst = new Set(["const"]);

export const directConst = new Set(["direct"]);

function check(values: Set<string>, expected: string): boolean {
  return values.has(expected);
}

function reassign(): void {
  aliasedVar = new Set(["var-after"]);
  aliasedLet = new Set(["let-after"]);
}

export {
  aliasedVar as VAR_SET,
  aliasedLet as LET_SET,
  aliasedConst as CONST_SET,
  check as checkAlias,
  reassign,
};
