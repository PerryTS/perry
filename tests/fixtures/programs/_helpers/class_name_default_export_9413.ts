// #9413 helper: `export default class {}` is the only spelling whose
// `.name` is "default" (ExportDeclaration NamedEvaluation), and it needs
// a second module to observe it.
export default class {}
export const namedDefault = class {};
