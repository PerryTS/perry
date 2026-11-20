import { $n, _n, $wide, _wide, $renamed, $renamedWide } from './provider.js';
import * as provider from './provider.js';

function check(actual, expected, label) {
  if (actual !== expected) throw new Error(label + ': ' + actual + ' !== ' + expected);
}

check($n(), 11, 'raw dollar export');
check(_n(new Map([['a', 3], ['b', 4]])), 7, 'underscore export');
check($renamed(), 11, 'renamed dollar export');
check($wide(1, 2, 3, 4, 5, 6), 32, 'six-argument dollar export');
check(_wide(), -1, 'underscore sibling');
check(provider.$n(), 11, 'namespace dollar export');
const callbacks = [$n, $renamed, $wide, $renamedWide];
check(callbacks[0](), 11, 'dollar function value');
check(callbacks[1](), 11, 'renamed function value');
check(callbacks[2](1, 2, 3, 4, 5, 6), 32, 'six-argument function value');
check(callbacks[3](1, 2, 3, 4, 5, 6), 32, 'six-argument renamed value');
console.log('PASS dollar/underscore export collision');
