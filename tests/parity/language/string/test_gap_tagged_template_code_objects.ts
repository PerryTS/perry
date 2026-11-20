// Tagged code fragments preserve cooked/raw strings, nested substitutions,
// mixed array items and string-hint object coercion.
function tag(s: TemplateStringsArray, ...args: any[]) { return [JSON.stringify(s),JSON.stringify(s.raw),args.map(String).join('|')].join(':'); }
class Code { text: string; constructor(text: string) { this.text = text; } toString() { return this.text; } }
const a = new Code('A');
console.log(tag`raw\n${a}\t${tag`x${a}y`}end`);
console.log(`${a}`, String(a), [a, 'B', 3, null, undefined].join('|'));
const obj = {toString() {return 'object';}};
console.log(tag`begin${obj}end`);
const primitive = {[Symbol.toPrimitive](hint: string) {return hint;}};
console.log(`${primitive}`, String(primitive));
