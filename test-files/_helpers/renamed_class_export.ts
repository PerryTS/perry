// Helper for test_gap_renamed_class_export_namespace.ts (#1758).
// Mirrors effect's `class Number$ extends make(numberKeyword) {}` +
// `export { Number$ as Number }` shape: a class whose static `ast` is
// INHERITED from a class-expression parent, re-exported under a renamed
// (and global-colliding) name.

export function make(a: any) {
  return class SchemaClass {
    static ast = a;
  };
}

class Number$ extends make({ _tag: "NumberKeyword" }) {}
class Widget$ extends make({ _tag: "WidgetKeyword" }) {}
export class DirectCls extends make({ _tag: "DirectKeyword" }) {}

// Renamed exports. `Number` deliberately collides with the JS global
// `Number` — pre-fix, `M.Number` resolved to the global constructor.
export { Number$ as Number, Widget$ as Widget };
