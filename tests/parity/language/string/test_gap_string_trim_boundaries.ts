const whitespace = [0x9, 0xa, 0xb, 0xc, 0xd, 0x20, 0xa0, 0x1680,
  0x2000, 0x2001, 0x2002, 0x2003, 0x2004, 0x2005, 0x2006, 0x2007,
  0x2008, 0x2009, 0x200a, 0x2028, 0x2029, 0x202f, 0x205f, 0x3000, 0xfeff];
function units(s: string): string {
  let out = '';
  for (let i = 0; i < s.length; i++) out += s.charCodeAt(i) + ',';
  return out;
}
for (const cp of whitespace) {
  const ws = String.fromCharCode(cp);
  const input = ws + 'a\ud800中😀\udfffz' + ws;
  console.log(cp, units(input.trim()), units(input.trimStart()), units(input.trimEnd()));
}
for (const input of ['', 'plain', ' \t\r\n ', '\tleft', 'right\n', '\u0085keep\u0085', '\u200bkeep\u200b']) {
  console.log(units(input.trim()), units(input.trimStart()), units(input.trimEnd()));
}
const interior = 'ä中😀Ö'.repeat(100);
let source = ' \t' + interior + '\n ';
const first = source.trim();
for (let i = 0; i < 30; i++) {
  console.log(i, source.trim() === interior, source.trimStart() === interior + '\n ', source.trimEnd() === ' \t' + interior);
}
let result = source.trim();
source += 'tail';
result += 'changed';
console.log(first === interior, result === interior + 'changed', source.trim() === interior + '\n tail');
let unique = 'prefix' + interior;
const unchanged = unique.trim();
unique += '!';
console.log(unchanged === 'prefix' + interior, unique === 'prefix' + interior + '!');
