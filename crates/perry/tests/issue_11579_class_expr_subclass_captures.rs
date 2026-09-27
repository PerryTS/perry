//! #11579: a capturing class expression evaluated more than once, then
//! extended by a declared class. Instances of each subclass must read the
//! captures of the evaluation their class extends, not the first evaluation's
//! (c3c587251's guarded class environment treated every unstamped instance as
//! belonging to the first evaluation).

mod support;

const SOURCE: &str = r#"
function makeLabelled(label: string) {
  return class {
    own() { return "own:" + label; }
    get tag() { return "tag:" + label; }
    static describe() { return "static:" + label; }
  };
}
class First extends makeLabelled("first") {}
class Second extends makeLabelled("second") {}
const Third = makeLabelled("third");
class Fourth extends Third {}
const first: any = new First();
const second: any = new Second();
const fourth: any = new Fourth();
console.log(first.own(), second.own(), fourth.own());
console.log(first.tag, second.tag, fourth.tag);
console.log((First as any).describe(), (Second as any).describe(), (Fourth as any).describe());
const direct: any = new Third();
console.log(direct.own(), direct.tag);
"#;

#[test]
fn subclass_instances_read_their_own_class_evaluations_captures() {
    assert_eq!(
        support::compile_and_run(SOURCE),
        concat!(
            "own:first own:second own:third\n",
            "tag:first tag:second tag:third\n",
            "static:first static:second static:third\n",
            "own:third tag:third\n",
        )
    );
}
