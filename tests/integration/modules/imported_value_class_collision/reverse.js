import { touch, OtherClass } from './types.js';
import { Shared } from './keys.js';

export function checkReverseOrder() {
  if (touch() !== 'loaded' || new OtherClass().value() !== 42) throw new Error('reverse class import');
  if (Shared.state('reverse').namespace !== 'state' || Shared.label !== 'object') {
    throw new Error('reverse object import');
  }
}
