import { store } from './accessor.js';
// The consumer deliberately does not import Registry/Settings. Its inlined
// accessor must carry that dependency across the second module boundary.
const first = store();
console.log('store type', typeof first);
if (typeof first !== 'object' || first === null) throw new Error('lost factory result');
first.perSource.set('fixture', 42);
if (store().perSource.get('fixture') !== 42) throw new Error('lost store identity');
console.log('PASS: transitive imported static receiver');
