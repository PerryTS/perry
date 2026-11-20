import { Shared } from './keys.js';
import { touch, OtherClass, makeInstance, ClassValue } from './types.js';
import { checkReverseOrder } from './reverse.js';
import { checkNamespace } from './namespace.js';

if (touch() !== 'loaded' || new OtherClass().value() !== 42) throw new Error('class import failed');
const key = Shared.state('settings');
if (key.namespace !== 'state' || key.id !== 'settings') throw new Error('object import failed');
if (Shared.label !== 'object') throw new Error('static field shadowed imported object');
const copy = Shared;
if (copy !== Shared || copy.state('copy').namespace !== 'state') throw new Error('imported value failed');
const method = Shared.state;
if (method('detached').namespace !== 'state') throw new Error('method value failed');
const computed = ['state'][0];
if (Shared[computed]('computed').namespace !== 'state') throw new Error('computed call failed');
if (Shared[computed](...['spread']).namespace !== 'state') throw new Error('spread call failed');
if (OtherClass.label !== 'class' || OtherClass.state('real').namespace !== 'wrong-class') {
  throw new Error('genuine imported class failed');
}
if (makeInstance().value() !== 42) throw new Error('factory return class metadata failed');
if (ClassValue.state('variable-class').namespace !== 'wrong-class') throw new Error('class-valued import failed');
checkReverseOrder();
checkNamespace();
console.log('PASS imported object is not shadowed by another module class');
