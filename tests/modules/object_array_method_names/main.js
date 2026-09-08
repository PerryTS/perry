function iterate(items, callback) {
  for (let i = 0; i < items.length; i++) callback(items[i]);
}
var utility = { forEach: iterate };
let output = '';
utility.forEach(['get', 'post'], method => { output += method + ';'; });
if (output !== 'get;post;') throw new Error('own forEach was bypassed: ' + output);
console.log('PASS: object forEach method');
