// Issue #5039: ink `<Text dimColor>` rendered `[object Object]` because
// chalk's boolean style modifiers never resolved. Three single-file roots
// (the fourth — `#`-subpath imports — is covered by resolver unit tests):
//
// 1. `closure_get_dynamic_prop`'s static-prototype walk ignored ACCESSOR
//    properties: chalk's styles are `{ get() {…} }` descriptors on
//    `createChalk.prototype` (and on a bare-function proto for builders),
//    and the getters must run with the ORIGINAL receiver so chalk's
//    `Object.defineProperty(this, styleName, {value: builder})` caches on
//    the instance instead of throwing "Cannot redefine property".
// 2. Codegen force-routed Annex B HTML-wrapper names (`bold`, `link`, …) on
//    any-typed receivers to String.prototype, so `chalk.bold(s)` coerced the
//    chalk closure to its source text and wrapped it in `<b>…</b>`.
// 3. `unescape_template` didn't decode `\xHH` / `\uHHHH` / `\u{…}`, so
//    ansi-styles' `` `\u001B[${code}m` `` produced 6 literal chars, not ESC.

// --- Part 1: getter on a function used as a set prototype (builder proto) ---
const fnProto: any = Object.defineProperties(() => {}, {
  dim: { get() { return (x: string) => 'DIM:' + x; } },
});
const fnTarget: any = (...s: any[]) => s.join(' ');
Object.setPrototypeOf(fnTarget, fnProto);
console.log('fn-proto read:', typeof fnTarget.dim);
console.log('fn-proto call:', fnTarget.dim('a'));

// --- Part 2: chalk's exact shape — getter on createChalk.prototype with a
// defineProperty self-cache, reached through Object.setPrototypeOf(fn, …) ---
const GENERATOR = Symbol('GENERATOR');
const STYLER = Symbol('STYLER');
const IS_EMPTY = Symbol('IS_EMPTY');

const ansiStyles: any = {
  dim: { open: '\u001B[2m', close: '\u001B[22m' },
  bold: { open: '\u001B[1m', close: '\u001B[22m' },
  cyan: { open: '\u001B[36m', close: '\u001B[39m' },
};

const styles: any = Object.create(null);

const applyOptions = (object: any, options: any = {}) => {
  object.level = options.level === undefined ? 1 : options.level;
};

const chalkFactory = (options?: any) => {
  const chalk = (...strings: any[]) => strings.join(' ');
  applyOptions(chalk, options);
  Object.setPrototypeOf(chalk, createChalk.prototype);
  return chalk;
};

function createChalk(options?: any): any {
  return chalkFactory(options);
}

Object.setPrototypeOf(createChalk.prototype, Function.prototype);

for (const [styleName, style] of Object.entries(ansiStyles)) {
  styles[styleName] = {
    get() {
      const builder = createBuilder(this, createStyler((style as any).open, (style as any).close, (this as any)[STYLER]), (this as any)[IS_EMPTY]);
      Object.defineProperty(this, styleName, { value: builder });
      return builder;
    },
  };
}

const proto: any = Object.defineProperties(() => {}, {
  ...styles,
  level: {
    enumerable: true,
    get() { return (this as any)[GENERATOR].level; },
    set(level: any) { (this as any)[GENERATOR].level = level; },
  },
});

const createStyler = (open: string, close: string, parent?: any) => {
  let openAll;
  let closeAll;
  if (parent === undefined) {
    openAll = open;
    closeAll = close;
  } else {
    openAll = parent.openAll + open;
    closeAll = close + parent.closeAll;
  }
  return { open, close, openAll, closeAll, parent };
};

const createBuilder = (self: any, _styler: any, _isEmpty: boolean) => {
  const builder = (...arguments_: any[]) => applyStyle(builder, (arguments_.length === 1) ? ('' + arguments_[0]) : arguments_.join(' '));
  Object.setPrototypeOf(builder, proto);
  builder[GENERATOR] = self;
  builder[STYLER] = _styler;
  builder[IS_EMPTY] = _isEmpty;
  return builder;
};

const applyStyle = (self: any, string: string) => {
  if (self.level <= 0 || !string) {
    return self[IS_EMPTY] ? '' : string;
  }
  let styler = self[STYLER];
  if (styler === undefined) {
    return string;
  }
  const { openAll, closeAll } = styler;
  return openAll + string + closeAll;
};

Object.defineProperties(createChalk.prototype, styles);

const chalk = createChalk();
console.log('dim :', JSON.stringify(chalk.dim('hello')));
console.log('bold:', JSON.stringify(chalk.bold('hello')));
console.log('cyan:', JSON.stringify(chalk.cyan('hello')));
console.log('nest:', JSON.stringify(chalk.dim.bold('hello')));
// Second read hits the defineProperty self-cache instead of the getter.
console.log('cached:', JSON.stringify(chalk.dim('again')));

// --- Part 3: Annex B HTML wrappers still work on real strings, including
// any-typed ones, while object methods with colliding names win dispatch ---
console.log('str bold typed:', 'x'.bold());
const anyStr: any = 'y';
console.log('str bold any  :', anyStr.bold());

// --- Part 4: template-literal escapes ---
const esc = (code: number) => `\u001B[${code}m`;
const e = esc(2);
console.log('tpl u-escape:', e.length, e.charCodeAt(0), e.charCodeAt(1));
const hexEsc = `\x1B[0m`;
console.log('tpl x-escape:', hexEsc.length, hexEsc.charCodeAt(0));
const braced = `\u{1F600}ok`;
console.log('tpl braced  :', braced.length, braced.codePointAt(0));
// Compared as char codes: Perry's JSON.stringify writes \u000c/\u0008 where
// Node uses the \f/\b short forms (pre-existing formatting gap, not #5039).
const misc = `a\0b\vc\fd\be`;
console.log('tpl misc    :', misc.length, Array.from(misc).map(c => c.charCodeAt(0)).join(','));
