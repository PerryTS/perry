// `new <ident>` where a lexical local shadows a same-named module-scope
// class: the LOCAL's value must be constructed, not the name-keyed class.
// Minified bundles hit this constantly — mysql2's chunk has a module-scope
// `class e {…}` (PacketParser) AND factory-local `let e = require(...)`
// (PoolConfig); `new e(opts)` constructed the wrong class silently.
class e {
  buffer: any[];
  packetHeaderLength: number;
  constructor(E: any, _?: number) { this.buffer = []; this.packetHeaderLength = _ || 4; }
}
class PoolConfigLike {
  connectionConfig: any;
  constructor(o: any) { this.connectionConfig = { host: o.host || 'x' }; }
}
// plain function-local shadow
function factory() {
  let e = PoolConfigLike;
  return new e({ host: 'db' });
}
const cfg = factory();
console.log('[s1]', typeof cfg.connectionConfig, cfg.connectionConfig && cfg.connectionConfig.host, 'buffer:', typeof (cfg as any).buffer);
// the module class itself still constructs correctly
const p = new e('x', 9);
console.log('[s2]', Array.isArray(p.buffer), p.packetHeaderLength);
// closure-captured shadow (the exact bundled-factory shape)
const mk = (function () {
  let e = PoolConfigLike;
  return function (o: any) { return new e(o); };
})();
const cfg2 = mk({ host: 'h2' });
console.log('[s3]', typeof cfg2.connectionConfig, cfg2.connectionConfig && cfg2.connectionConfig.host);
// param shadow
function build(e: any) { return new e({ host: 'h3' }); }
const cfg3 = build(PoolConfigLike);
console.log('[s4]', cfg3.connectionConfig && cfg3.connectionConfig.host);
