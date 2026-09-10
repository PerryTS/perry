const value: any = JSON.parse('{"a":1,"b":2}');
const parsed: any = JSON.parse('[]');
console.log('parsed', JSON.stringify(value, parsed));
const literal: any = [];
console.log('literal', JSON.stringify(value, literal));
console.log('missing', JSON.stringify(value, ['missing']));
