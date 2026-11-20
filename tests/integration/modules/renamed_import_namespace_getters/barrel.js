import { $value, $object, increment } from './provider.js';
export function _value(value) { return value + 100; }
export { $value as count, $object as item, increment };
