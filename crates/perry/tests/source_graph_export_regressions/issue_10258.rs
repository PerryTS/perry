//! Constructor contracts must agree across modules for every heritage form.
//! Expected stdout is captured from Bun, not inferred from Perry's output.

use super::{compile_and_run_with_all_llvm_trace, write};

#[test]
fn imported_default_ctor_with_runtime_parent_forwards_args() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "core.ts",
        r#"export const YieldableError = (function () { class YieldableError extends globalThis.Error {}; return YieldableError })()
const plainArgsSymbol = Symbol.for("effect/Data/Error/plainArgs")
export const Error = (function () {
  return class Base extends YieldableError {
    constructor(args?: any) {
      super(args?.message, args?.cause ? { cause: args.cause } : undefined)
      if (args) { Object.assign(this, args); Object.defineProperty(this, plainArgsSymbol, { value: args, enumerable: false }) }
    }
  }
})()
export const TaggedError = (tag: string) => { class Base extends Error { readonly _tag = tag }; return Base as any }
export const Plain = (function () { return class P { constructor(a?: any, b?: any, c?: any) { (this as any).sum = [a, b, c] } } })()
export class ImportedParent {
  first: any
  second: any
  constructor(a: any, b: any) { this.first = a; this.second = b }
}
"#,
    );
    write(
        dir.path(),
        "data.ts",
        r#"import * as core from "./core"
export const Error = core.Error
export const TaggedError = core.TaggedError
export const Plain = core.Plain
export { ImportedParent as RealParent } from "./core"
"#,
    );
    write(
        dir.path(),
        "platform.ts",
        r#"import * as Data from "./data"
import { Error as NamedErr, RealParent as AliasedParent } from "./data"
export class Empty extends (Data.Error as any) {}
export class WithGetter extends (Data.Error as any) { get message() { return "G:" + (this as any)._tag } }
export class WithMethod extends (Data.Error as any) { describe() { return (this as any).module } }
export class Explicit extends (Data.Error as any) { constructor(a: any) { super(a) } }
export class ThreeArgs extends (Data.Plain as any) {}
export class MemberHeritage extends Data.Error {}
export class MemberHeritageGetter extends Data.Error { get message() { return "MH" } }
export class NamedHeritage extends NamedErr {}
class LocalParent {
  first: any
  second: any
  constructor(a: any, b: any) { this.first = a; this.second = b }
}
class LocalMiddle extends LocalParent {}
export class LocalChild extends LocalMiddle {}
export { LocalChild as RenamedChild }
export class ImportedChild extends AliasedParent {}
export class MemberImportedChild extends Data.RealParent {}
export class PlatformError extends Data.TaggedError("PlatformError") {
  constructor(reason: any) { super({ reason, cause: reason.cause }) }
}
export const makeLocal = (o: any) => new WithGetter(o)
export const makeMember = (o: any) => new MemberHeritage(o)
export const makeMemberGetter = (o: any) => new MemberHeritageGetter(o)
export const makeNamed = (o: any) => new NamedHeritage(o)
"#,
    );
    write(
        dir.path(),
        "main.ts",
        r#"import { Empty, WithGetter, WithMethod, Explicit, ThreeArgs, makeLocal, MemberHeritage, MemberHeritageGetter, NamedHeritage, LocalChild, ImportedChild, MemberImportedChild } from "./platform"
import { RenamedChild } from "./platform"
import * as P from "./platform"
import * as Data from "./data"
const o = () => ({ _tag: "NotFound", module: "FS" })
const show = (e: any) => [e._tag, e.module, e instanceof Error].join(",")
const fields = (e: any) => JSON.stringify({ tag: e._tag, module: e.module, keys: Object.keys(e) })
const pair = (e: any) => JSON.stringify([e.first, e.second])
console.log(show(new Empty(o())), show(new WithGetter(o())), new WithGetter(o()).message)
console.log(show(new WithMethod(o())), new WithMethod(o()).describe(), show(new Explicit(o())))
console.log(show(new P.Empty(o())), show(makeLocal(o())), JSON.stringify((new ThreeArgs(1, 2, 3) as any).sum))
console.log("member", fields(new MemberHeritage(o())))
console.log("getter", fields(new MemberHeritageGetter(o())), new MemberHeritageGetter(o()).message)
console.log("named", fields(new NamedHeritage(o())))
console.log("namespace", fields(new P.MemberHeritage(o())))
console.log("defining module", fields(P.makeMember(o())), fields(P.makeMemberGetter(o())), fields(P.makeNamed(o())))
class LocalRuntime extends Data.Error {}
console.log("local runtime", fields(new LocalRuntime(o())))
console.log("local parent", pair(new LocalChild("local", 2)), pair(new P.LocalChild("namespace", 22)))
console.log("imported parent", pair(new ImportedChild("imported", 3)), pair(new MemberImportedChild("member", 33)))
console.log("barrel", pair(new RenamedChild("renamed", 4)))
const wrapped: any = new P.PlatformError(new Empty({ ...o(), cause: new Error("c") }))
console.log("wrapped", JSON.stringify({ tag: wrapped._tag, reasonTag: wrapped.reason._tag, keys: Object.keys(wrapped), cause: wrapped.cause.message }))
class Sub extends Empty {}
class MemberSub extends MemberHeritage {}
class ResolvedSub extends ImportedChild {}
console.log(show(new Sub(o())), fields(new MemberSub(o())), pair(new ResolvedSub("sub", 5)))
"#,
    );
    let (stdout, ir) = compile_and_run_with_all_llvm_trace(dir.path(), "main.ts");
    assert_eq!(stdout, include_str!("issue_10258.stdout"));

    // Behavioral output alone cannot detect an over-declared signature on
    // platforms that happen to ignore the surplus argument registers.
    for (name, params) in [
        ("Empty", 8),
        ("MemberHeritage", 8),
        ("MemberHeritageGetter", 8),
        ("NamedHeritage", 8),
        ("ThreeArgs", 8),
        ("LocalChild", 2),
        ("ImportedChild", 2),
        ("MemberImportedChild", 2),
    ] {
        let symbol = format!("__{name}_constructor(");
        let signatures: Vec<_> = ir
            .lines()
            .filter(|line| {
                (line.starts_with("define ") || line.starts_with("declare "))
                    && line.contains(&symbol)
            })
            .collect();
        assert!(
            signatures.iter().any(|line| line.starts_with("define ")),
            "missing definition: {symbol}"
        );
        assert!(
            signatures.iter().any(|line| line.starts_with("declare ")),
            "missing import: {symbol}"
        );
        for line in signatures {
            let args = line
                .split_once(&symbol)
                .unwrap()
                .1
                .split(')')
                .next()
                .unwrap();
            assert_eq!(args.matches("double").count(), params + 1, "{line}");
        }
    }
}
