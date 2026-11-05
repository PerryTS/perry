use std::{env, fs, path::Path};

pub fn emit(workspace: &Path) {
    let rules = workspace.join("npm/archive-kinds.rules");
    println!("cargo:rerun-if-changed={}", rules.display());
    let mut code = String::from("fn archive_kind(stem: &str) -> Option<&'static str> {\n");
    for line in fs::read_to_string(rules)
        .expect("archive kind rules")
        .lines()
    {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (kind, pattern) = line.split_once(' ').expect("kind and archive stem");
        let pattern = pattern.trim();
        let predicate = if let Some(prefix) = pattern.strip_suffix('*') {
            format!("stem.starts_with({prefix:?})")
        } else {
            format!("stem == {pattern:?}")
        };
        code.push_str(&format!("if {predicate} {{ return Some({kind:?}); }}\n"));
    }
    code.push_str("None\n}\n");
    fs::write(
        Path::new(&env::var_os("OUT_DIR").unwrap()).join("archive_kinds.rs"),
        code,
    )
    .expect("generated archive classifier");
}
