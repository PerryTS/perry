// #4995: bare-specifier `events` default/namespace imports must yield the
// same working EventEmitter as the `node:events` named import. signal-exit
// (transitive ink dep) does `var EE = require('events'); new EE()` and calls
// `.setMaxListeners(Infinity)` on the instance during module init.
import { EventEmitter } from 'node:events';
import EEBare from 'events';
import EENode from 'node:events';
import * as evNs from 'events';

// Named node: import — the previously-working reference point.
const named = new EventEmitter();
console.log('named ctor:', typeof EventEmitter);
console.log('named setMaxListeners:', typeof named.setMaxListeners);

// Default import, bare specifier (the signal-exit shape).
const DefaultBare: any = EEBare;
console.log('default bare ctor:', typeof DefaultBare);
const db = new DefaultBare();
console.log('default bare on:', typeof db.on);
console.log('default bare once:', typeof db.once);
console.log('default bare emit:', typeof db.emit);
console.log('default bare removeListener:', typeof db.removeListener);
console.log('default bare setMaxListeners:', typeof db.setMaxListeners);
db.setMaxListeners(Infinity);
console.log('default bare getMaxListeners:', db.getMaxListeners());
let hits = 0;
db.on('sig', (n: number) => {
  hits += n;
});
db.emit('sig', 2);
db.emit('sig', 3);
console.log('default bare emit hits:', hits);

// Default import, node: specifier.
const DefaultNode: any = EENode;
const dn = new DefaultNode();
console.log('default node setMaxListeners:', typeof dn.setMaxListeners);
dn.setMaxListeners(20);
console.log('default node getMaxListeners:', dn.getMaxListeners());

// Namespace import, bare specifier.
const ns: any = evNs;
console.log('ns EventEmitter:', typeof ns.EventEmitter);
console.log('ns default:', typeof ns.default);
const nse = new ns.EventEmitter();
console.log('ns setMaxListeners:', typeof nse.setMaxListeners);
nse.once('x', () => {
  console.log('ns once fired');
});
nse.emit('x');
nse.emit('x');
console.log('ns listenerCount after once:', nse.listenerCount('x'));

// captureRejections option must reach the constructor on the dynamic path.
const opt = new DefaultBare({ captureRejections: true });
console.log('options instance on:', typeof opt.on);
