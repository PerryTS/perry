// Regression for #341: class field strongly typed as
// `Database.Database` (or any namespace-qualified native instance type)
// SIGSEGV'd when accessed via `this.<field>.method()` after the inliner
// copied the method body into the caller and substituted `this` with
// the receiver local.
//
// Pre-fix: the codegen's `receiver_class_name(this.db)` returned
// "Database" (the field's declared type — a TS type from
// @types/better-sqlite3, not a Perry class). The IC fast path then
// dispatched as if the receiver were a real heap object, deref'ing
// `obj-8` for the GcHeader byte. For a small handle id (1..n), `1-8 =
// -7` SIGSEGV'd.
//
// Fix: js_transform.rs builds a per-class `field → (module, native_class)`
// map (from constructor `this.<field> = new Database(...)` patterns +
// field initializers) and rewrites both `this.<field>.method(...)` and
// `<localGet>.<field>.method(...)` (the post-inline shape) to
// `NativeMethodCall` so dispatch routes through the runtime's native
// table instead of the class IC.
import Database from 'better-sqlite3';

class S {
  private db: Database.Database;

  constructor(path: string) {
    this.db = new Database(path);
  }

  count(): number {
    // Chained shape — what the issue's repro used. Triggers the
    // post-inline `<local>.<field>.<method>()` path because the
    // inliner copies the method body into the caller, substitutes
    // `this` with the `s` local, and produces `s.db.prepare(...).get()`
    // at the call site.
    return (this.db.prepare('SELECT 1 AS n').get() as any).n;
  }

  countMultiStmt(): number {
    // Multi-statement form — exercises the tracked-receiver path
    // through a separate local. Should still work after the fix.
    const stmt = this.db.prepare('SELECT 2 AS n');
    const row: any = stmt.get();
    return row.n;
  }
}

const s = new S(':memory:');
console.log('count =', s.count());
console.log('countMultiStmt =', s.countMultiStmt());
