// #11446: map/flatMap results have their callbacks' element types.
class Matcher11446 {
  value: string;
  constructor(value: string) { this.value = value; }
  match(value: string): boolean { return this.value === value; }
}
function makeMatcher11446(value: string): any {
  return new Matcher11446(value);
}

const source11446 = ['yes', 'no'];
const mapped11446 = source11446.map(value => makeMatcher11446(value));
const flat11446 = source11446.flatMap(value => [makeMatcher11446(value)]);
console.log('direct', makeMatcher11446('yes').match('yes'));
console.log('map', mapped11446[0].match('yes'), mapped11446[1].match('yes'));
console.log('flatMap', flat11446[0].match('yes'), flat11446[1].match('yes'));
console.log('chain', mapped11446.filter(value => true).slice()[0].match('yes'));

const lengths11446 = source11446.map((value: string): number => value.length);
const flatLengths11446 = source11446.flatMap((value: string): number[] => [value.length]);
console.log('numbers', lengths11446[0] + lengths11446[1], flatLengths11446.join(','));
const strings11446 = [1, 2].map(value => 'word' + value);
console.log('strings', strings11446[0].toUpperCase(), strings11446[1].match(/word/)?.[0]);
console.log('preserved', source11446.filter(value => true).slice().reverse().join(','));
