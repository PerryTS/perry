// Cross-class ASCII signs match Node; exact punctuation ordering is deliberately
// outside Perry's table-free comparator. Test every printable punctuation and
// every ASCII digit/letter, in both directions and after a Unicode prefix.
const punctuation = '!"#$%&\'()*+,-./:;<=>?@[\\]^_`{|}~';
const alphanumeric = '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz';
let before = 0, after = 0;
for (let i = 0; i < punctuation.length; i++) {
  for (let j = 0; j < alphanumeric.length; j++) {
    const a = 'items/' + punctuation[i];
    const b = 'items/' + alphanumeric[j];
    if (a.localeCompare(b, 'en') < 0) before++;
    if (b.localeCompare(a, 'en') > 0) after++;
  }
}
console.log(before, after, punctuation.length * alphanumeric.length);
console.log('items/{id}'.localeCompare('items/bulk', 'en'));
console.log('café/{id}'.localeCompare('café/bulk', 'en'));
console.log('ΟΣ/{id}'.localeCompare('ος/bulk', 'en'));
// Negative controls: tertiary case, canonical equivalence, numeric mode,
// and distinct lone surrogates must keep their existing signs.
console.log('a'.localeCompare('A'), 'a'.localeCompare('b'));
console.log('é'.localeCompare('e\u0301'));
console.log('file10'.localeCompare('file9', 'en', {numeric: true}));
console.log('\ud800'.localeCompare('\ud801'));
