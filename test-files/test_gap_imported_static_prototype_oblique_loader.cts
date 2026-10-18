const first = require('./fixtures/awb2-static-proto/reload.cjs');
const ctorKey = ['con', 'structor'].join('');
const dynamic = (() => 0)[ctorKey];
const reload = dynamic('filename', "const m = process.getBuiltinModule('module'); const r = m.createRequire(filename); const p = r.resolve('./fixtures/awb2-static-proto/reload.cjs'); delete m._cache[p]; return r(p);");
const second = reload(__filename);
console.log('identity', first !== second, first.Class !== second.Class, first.Class.prototype !== second.Class.prototype);
first.Class.prototype.own = () => 71;
console.log('values', first.make().run(), second.make().run());
