// #12309: dependency-free reduction of Effect 4.0.2 SchemaAST.Union.
class ASTNodeImpl {
  annotations;
  constructor(annotations) { this.annotations = annotations; }
  toString() { return '<' + this._tag + '>'; }
}
// Recursion keeps the constructor reference in its own compiled function.
function makeUnion(types, depth) {
  if (depth > 0) return makeUnion(types, depth - 1);
  return new Union(types, 'any', 'ann');
}
export const Union = class extends ASTNodeImpl {
  _tag = 'Union';
  types;
  options;
  constructor(types, options, annotations) {
    super(annotations);
    this.types = types;
    this.options = options;
  }
  getParser() { return this.types.join(','); }
};
const ast: any = makeUnion(['a', 'b'], 1);
console.log(ast._tag, ast.types?.length, ast.options, ast.annotations);
console.log(typeof ast.getParser, typeof ast.toString);
if (typeof ast.getParser === 'function') console.log(ast.getParser(), ast.toString());
