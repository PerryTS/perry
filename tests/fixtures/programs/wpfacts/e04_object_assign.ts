// E4: Object.assign writes keys of a source into the target.
// WPFACTS: fact F1 readonly Cfg.mode = 0
// WPFACTS: kind Cfg.mode has str
class Cfg {
  mode: number = 0;
}
const c = new Cfg();
Object.assign(c, { mode: "fast" });
console.log(c.mode);
