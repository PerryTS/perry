const first = require('./fixtures/awb2-static-proto/reload.cjs');
const path = require.resolve('./fixtures/awb2-static-proto/reload.cjs');
delete require.cache[path];
const second = require('./fixtures/awb2-static-proto/reload.cjs');
console.log('reload', first !== second, first.Class !== second.Class, first.Class.prototype !== second.Class.prototype);
first.Class.prototype.own = () => 23;
console.log('independent', first.make().run(), second.make().run());
