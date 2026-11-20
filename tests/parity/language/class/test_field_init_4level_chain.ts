// Refs #420 / #631-followup: 4-level inheritance with intermediate-class
// field initializer that depends on parent body state. Pre-fix Perry's
// FieldInitMode::AncestorsOnly ran ALL ancestor fields up-front before
// any body executed, so the intermediate class's field init read this.config
// before Column's body had set it.
//
// Mirrors drizzle-sqlite's `SQLiteInteger ← SQLiteBaseInteger ← SQLiteColumn
// ← Column` shape:
//   - Column's ctor sets `this.config`.
//   - SQLiteBaseInteger has no own ctor, but has a field initializer
//     `autoIncrement = this.config.autoIncrement`.
//   - SQLiteInteger has its own ctor that calls super().
//
// Per ECMAScript spec, SQLiteBaseInteger's fields run after its (default)
// super() returns — at which point Column's body has set this.config.
class Column {
    table: any;
    config: any;
    constructor(table: any, config: any) {
        this.table = table;
        this.config = config;
    }
}
class SQLiteColumn extends Column {
    constructor(table: any, config: any) {
        super(table, config);
    }
}
class SQLiteBaseInteger extends SQLiteColumn {
    autoIncrement = this.config.autoIncrement;
}
class SQLiteInteger extends SQLiteBaseInteger {
    constructor(table: any, config: any) {
        super(table, config);
    }
}
const c1 = new SQLiteInteger({}, { autoIncrement: true, name: "id" });
console.log("autoIncrement:", c1.autoIncrement);
console.log("config.name:", c1.config.name);

// Same pattern but leaf has no own ctor.
class SQLiteInteger2 extends SQLiteBaseInteger {}
const c2 = new SQLiteInteger2({}, { autoIncrement: false, name: "x" });
console.log("autoIncrement2:", c2.autoIncrement);
console.log("config2.name:", c2.config.name);
