//! A `const` alias of `this` in a class method is the `this` group: dayjs's
//! `init` shape (`const s = this; s.$y = …`) takes late slots on the class's
//! family instead of an overflow table per instance (#10496).
use super::*;

fn lower(src: &str) -> HirModule {
    let src = src.to_owned();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed =
                perry_parser::parse_typescript_with_cache(&src, "test.ts", &mut cache).unwrap();
            perry_hir::lower_module(&parsed.module, "test", "test.ts").unwrap()
        })
        .unwrap()
        .join()
        .unwrap()
}

fn this_group_stores(hir: &HirModule, class: &str) -> Vec<String> {
    let id = hir.classes.iter().find(|c| c.name == class).unwrap().id;
    module_groups(hir)
        .into_iter()
        .filter(|(c, _)| *c == Some(id))
        .flat_map(|(_, g)| g.stores)
        .collect()
}

#[test]
fn a_const_alias_of_this_stores_into_the_this_group() {
    let hir = lower(
        "class Dayjs { [k: string]: any; $L = \"en\"; init(t: number) { const s: any = this; s.$y = t; s.$M = t + 1; } }\n\
         class Direct { [k: string]: any; init(t: number) { this.$y = t; } }\n\
         class Rebound { [k: string]: any; init(t: number, o: any) { let s: any = this; s = o; s.$y = t; } }\n",
    );
    assert_eq!(this_group_stores(&hir, "Dayjs"), ["$y", "$M"]);
    assert_eq!(this_group_stores(&hir, "Direct"), ["$y"]);
    // A reassignable binding is not `this`.
    assert!(this_group_stores(&hir, "Rebound").is_empty());
}
