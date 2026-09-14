//! `new ImportedClass(args)` for an exported class with no own constructor whose
//! parent is a runtime value must forward the arguments (#10258). The defining
//! module synthesizes a fixed-arity forwarding constructor; the importer used to
//! declare it with zero parameters and dropped every argument. effect v4's
//! `PlatformError.SystemError extends Data.Error {}` has this shape.

use super::{compile_and_run, write};

#[test]
fn imported_default_ctor_with_runtime_parent_forwards_args() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "core.ts",
        "export const YieldableError = (function () { class YieldableError extends globalThis.Error {}; return YieldableError })()\n\
         export const Error = (function () {\n\
         \x20 return class Base extends YieldableError {\n\
         \x20   constructor(args?: any) { super(args?.message); if (args) Object.assign(this, args) }\n\
         \x20 }\n\
         })()\n\
         export const Plain = (function () { return class P { constructor(a?: any, b?: any, c?: any) { (this as any).sum = [a, b, c] } } })()\n",
    );
    write(dir.path(), "data.ts", "import * as core from \"./core\"\nexport const Error = core.Error\nexport const Plain = core.Plain\n");
    write(
        dir.path(),
        "platform.ts",
        "import * as Data from \"./data\"\n\
         export class Empty extends (Data.Error as any) {}\n\
         export class WithGetter extends (Data.Error as any) { get message() { return \"G:\" + (this as any)._tag } }\n\
         export class WithMethod extends (Data.Error as any) { describe() { return (this as any).module } }\n\
         export class Explicit extends (Data.Error as any) { constructor(a: any) { super(a) } }\n\
         export class ThreeArgs extends (Data.Plain as any) {}\n\
         export const makeLocal = (o: any) => new WithGetter(o)\n",
    );
    write(
        dir.path(),
        "main.ts",
        "import { Empty, WithGetter, WithMethod, Explicit, ThreeArgs, makeLocal } from \"./platform\"\n\
         import * as P from \"./platform\"\n\
         const o = () => ({ _tag: \"NotFound\", module: \"FS\" })\n\
         const show = (e: any) => [e._tag, e.module, e instanceof Error].join(\",\")\n\
         console.log(show(new Empty(o())), show(new WithGetter(o())), new WithGetter(o()).message)\n\
         console.log(show(new WithMethod(o())), new WithMethod(o()).describe(), show(new Explicit(o())))\n\
         console.log(show(new P.Empty(o())), show(makeLocal(o())), JSON.stringify((new ThreeArgs(1, 2, 3) as any).sum))\n\
         class Sub extends Empty {}\n\
         console.log(show(new Sub(o())))\n",
    );
    assert_eq!(
        compile_and_run(dir.path(), "main.ts"),
        "NotFound,FS,true NotFound,FS,true G:NotFound\n\
         NotFound,FS,true FS NotFound,FS,true\n\
         NotFound,FS,true NotFound,FS,true [1,2,3]\n\
         NotFound,FS,true\n"
    );
}
