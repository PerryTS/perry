// Refs v0.5.755: when a class extends a class from a DIFFERENT module
// AND the derived class's constructor body has `this.X = Y` assignments
// after `super(...)`, those assignments now persist. Pre-fix, the
// codegen at lower_call's no-own-ctor path re-applied field
// initializers AFTER the cross-module constructor returned (because
// the imported-class stub had no `constructor` field visible to the
// caller's module), overwriting whatever the source ctor body just
// set with the field's declared default (or undefined). Drizzle's
// BetterSQLiteSession's `this.client = client; this.schema = schema;
// this.logger = options.logger ?? new NoopLogger()` after
// super(dialect) was the load-bearing site.
import { Parent } from "./_helpers/cross_module_ctor_parent.ts";

class Derived extends Parent {
    client: any;
    schema: any;
    logger: any;
    constructor(client: any, dialect: any, schema: any, options: any = {}) {
        super(dialect);
        this.client = client;
        this.schema = schema;
        this.logger = options.logger ?? "default-logger";
    }
}

const d = new Derived("CLIENT-X", "DIALECT-Y", "SCHEMA-Z");
console.log("dialect:", (d as any).dialect);
console.log("client:", d.client);
console.log("schema:", d.schema);
console.log("logger:", d.logger);
