import { count, item, increment, _value } from './barrel.js';
function check(actual, expected) {
  if (actual !== expected) throw new Error('renamed import getter: ' + actual + ' !== ' + expected);
}
const namespaces = [await import('./barrel.js')];
const bridges = [await import('./bridge.js')];
check(namespaces[0].count, 5);
check(namespaces[0].item === item, true);
check(namespaces[0].item.name, 'original');
check(bridges[0].forwarded, 5);
check(bridges[0].forwardedItem === item, true);
check(_value(2), 102);
increment();
check(count, 6);
check(namespaces[0].count, 6);
check(bridges[0].forwarded, 6);
console.log('PASS renamed imported namespace getters');
