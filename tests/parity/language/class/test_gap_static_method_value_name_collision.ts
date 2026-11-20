// #7689: a static method extracted as a VALUE off the constructor must stay
// the static when an instance method shares its name (marked's Lexer.lex /
// Parser.parse shape: `const lexer2 = _Lexer.lex; lexer2(src, opt)`).
const defaults: any = { pedantic: false, gfm: true };

const Lexer = class __Lexer {
  options: any;
  constructor(o?: any) {
    this.options = o || defaults;
  }
  static lex(src: string, o?: any) {
    const l = new (__Lexer as any)(o);
    return l.lex(src);
  }
  static lexInline(src: string, o?: any) {
    const l = new (__Lexer as any)(o);
    return "inline:" + l.lex(src);
  }
  lex(src: string): string {
    return this.blockTokens(src);
  }
  blockTokens(src: string): string {
    if (this.options.pedantic) return "PED";
    return "ok:" + src.length;
  }
};

// Extracted unbound through a ternary, exactly like marked's parseMarkdown.
function run(blockType: boolean, opt: any): string {
  const lexer2 = blockType ? (Lexer as any).lex : (Lexer as any).lexInline;
  return lexer2("# hi", opt);
}
console.log(run(true, { ...defaults }));
console.log(run(false, { ...defaults }));

// Plain extraction without the ternary.
const f = (Lexer as any).lex;
console.log(f("# hello", { ...defaults }));

// Class DECLARATION form of the same collision.
class Parser {
  options: any;
  constructor(o?: any) {
    this.options = o || defaults;
  }
  static parse(tokens: string[], o?: any) {
    const p = new Parser(o);
    return p.parse(tokens);
  }
  parse(tokens: string[]): string {
    return "parsed:" + tokens.length + ":" + String(this.options.gfm);
  }
}
const g = (Parser as any).parse;
console.log(g(["a", "b"], { ...defaults }));

// Direct calls on the class keep working.
console.log((Lexer as any).lex("# direct", { ...defaults }));
console.log(Parser.parse(["x"], { ...defaults }));

// The prototype ref still names the INSTANCE method.
const pm = (Parser.prototype as any).parse;
console.log(typeof pm);
