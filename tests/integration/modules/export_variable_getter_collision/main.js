import { Tool, _item } from './provider.js';
function check(actual, expected) {
  if (actual !== expected) throw new Error('getter collision: ' + actual + ' !== ' + expected);
}
check(Tool.name, 'SendFile');
check(_item({ name: 'ordinary function' }), 'ordinary function');
// Dynamic import requires the runtime live-getter path also used by require().
// Static namespace imports can instead be projected from the named imports.
const namespaces = [await import('./provider.js')];
check(namespaces[0].Tool.name, 'SendFile');
check(namespaces[0].Tool === Tool, true);
console.log('PASS dollar variable/underscore function namespace getter');
