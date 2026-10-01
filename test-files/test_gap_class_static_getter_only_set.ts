// #11521: a write to a getter-only static accessor is rejected. Strict
// [[Set]] (module code) throws a TypeError; Reflect.set reports false. The
// value is unchanged either way, on the class and on a subclass.
class Cfg {
  static get version() { return 3; }
  static get label() { return "cfg:" + (this as any).name; }
  static set label(_v: string) { Cfg.writes++; }
  static writes = 0;
}
class Sub extends Cfg {}

function attempt(what: string, fn: () => void) {
  try {
    fn();
    console.log(what, "accepted");
  } catch (e) {
    console.log(what, (e as Error).constructor.name, e instanceof TypeError);
  }
}

attempt("Cfg.version", () => { (Cfg as any).version = 4; });
attempt("Sub.version", () => { (Sub as any).version = 4; });
attempt("Cfg[k]", () => { const k = "version"; (Cfg as any)[k] = 5; });
console.log(Cfg.version, (Sub as any).version);
console.log(Reflect.set(Cfg, "version", 6), Reflect.set(Sub, "version", 6), Cfg.version);
// A setter half accepts the write (and runs), on the class and a subclass.
attempt("Cfg.label", () => { (Cfg as any).label = "x"; });
attempt("Sub.label", () => { (Sub as any).label = "y"; });
console.log(Reflect.set(Cfg, "label", "z"), Cfg.writes, Cfg.label, (Sub as any).label);
// The getter-only refusal does not create an own data property.
console.log(Object.getOwnPropertyNames(Sub).includes("version"), typeof Object.getOwnPropertyDescriptor(Cfg, "version")!.get);
