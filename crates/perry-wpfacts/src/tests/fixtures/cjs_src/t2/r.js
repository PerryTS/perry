const NONE = null;
class R {
  append(p, q) {
    p.link = NONE;
    if (NONE === q) return p;
    let m = q;
    while (NONE !== m.link) m = m.link;
    m.link = p;
    return q;
  }
}
class S extends R {
  constructor(v) { super(); this.v = v; this.link = NONE; }
  get() { return this.v; }
  static make(v) { return new S(v); }
}
class T extends R {
  constructor() { super(); this.k = S; }
  run() { const a = this.k.make(1); const b = S.make(2); a.append(b, NONE); return a.get() + b.get(); }
}
exports.newInstance = () => new T();
