const hooks = require('node:async_hooks');
const bound = hooks.AsyncResource.bind(value => value + 1);
if (bound(41) !== 42) throw new Error('require static bind callback');
console.log('PASS: require AsyncResource.bind');
