// TypeScript-from-source shapes (checker.ts / parser.ts): enums are runtime
// values, an exported enum is a namespace member, and namespaces / enums are
// read by functions declared above them.

function readJsx(): string { return JsxNames.Element + "|" + JsxNames.count; }
function parseKind(n: number): number { return Parser.parse(n); }

enum Color { Red, Green = 4, Blue }
enum Label { A = "a", B = "b" }

const colorObj: any = Color;
console.log(colorObj.Green, colorObj[5], Object.keys(colorObj).join(","));
console.log(Color === colorObj, Color[Color.Blue], Object.keys(Label).join(","));

function formatEnum(value: number, enumObject: any): string {
    for (const key of Object.keys(enumObject)) {
        if (enumObject[key] === value) return key;
    }
    return "?";
}
console.log(formatEnum(4, Color), formatEnum(9, Color));

namespace Parser {
    export function parse(n: number): number { return n === 1 ? Ctx.First : Ctx.Other; }
    const enum Ctx { First = 10, Other = 20 }
    export enum Tristate { False, True, Unknown }
    export function tri(): Tristate { return Tristate.Unknown; }
}
console.log(parseKind(1), parseKind(2), Parser.Tristate.Unknown, Parser.Tristate[1], Parser.tri());

namespace JsxNames {
    export const Element = "Element";
    export let count = 1;
    export function bump(): void { count++; }
}
console.log(readJsx());
JsxNames.bump();
console.log(readJsx(), JsxNames.count);

namespace Outer {
    export function viaInner(n: number): string { return Inner.describe(n) + "/" + Inner.tag; }
    export namespace Inner {
        export const tag = "inner";
        export function describe(n: number): string { return "n=" + n; }
    }
}
console.log(Outer.viaInner(3), Outer.Inner.tag);

// A namespace body is its own scope (parser.ts: module `let SourceFileConstructor`
// beside `namespace Parser { var SourceFileConstructor }`).
let SourceCtor: any = "outer";
namespace Scoped {
    var SourceCtor: any;
    var factory = { make: () => SourceCtor };
    export function init(): void { SourceCtor = "inner"; }
    export function make(): any { return factory.make(); }
}
Scoped.init();
console.log(Scoped.make(), SourceCtor);

// Namespace functions are their own bindings (parser.ts: module-level
// `createSourceFile` calls `Parser.parseSourceFile`, whose namespace has its
// own non-exported `createSourceFile`).
export function createSource(text: string): string { return Parse.parseSource(text); }
export function parseLabel(text: string): string { return "module:" + Parse.parseLabel(text); }
namespace Parse {
    var state = "";
    export function parseSource(text: string): string { init(text); return createSource(); }
    export function parseLabel(text: string): string { return "ns:" + text; }
    function init(text: string): void { state = text; }
    function createSource(): string { return "source(" + state + ")"; }
}
console.log(createSource("x"), parseLabel("y"), createSource.name);

// Constant enum expressions, as tsc evaluates them (types.ts SyntaxKind / NodeFlags).
enum Kind { Unknown, EndOfFile, Comma = 28, Dot, LastPunct = Dot, FirstToken = Unknown, LastToken = LastPunct, Next }
enum Flags { None = 0, Let = 1 << 0, Const = 1 << 1, Using = 1 << 2, BlockScoped = Let | Const | Using, All = ~0, High = 1 << 31, Shifted = High >>> 28, Mixed = Flags.Let + 10, Str = "pre" + "fix", Tpl = `t` }
enum Derived { A = Flags.BlockScoped * 2, B = Kind["LastToken"] - 1, C = (Kind.Comma + 2) % 7, D = -Kind.Dot }
console.log(Kind.FirstToken, Kind.LastToken, Kind.Next, Kind[29], Flags.BlockScoped, Flags.All, Flags.High, Flags.Shifted, Flags.Mixed, Flags.Str, Flags.Tpl);
console.log(Derived.A, Derived.B, Derived.C, Derived.D, Object.keys(Derived).join(","));
