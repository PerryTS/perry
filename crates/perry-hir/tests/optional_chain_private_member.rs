use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

fn lower_src(src: &str) -> anyhow::Result<perry_hir::Module> {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "optional_chain_private_member.js", &mut cache)?;
    lower_module(&parsed.module, "test", "optional_chain_private_member.js")
}

#[test]
fn optional_chain_on_private_member_lowers() {
    lower_src(
        r#"
        class Example {
          #state = { type: "ready" };
          read() {
            return this.#state?.type;
          }
        }
        "#,
    )
    .expect("this.#state?.type should lower");
}

#[test]
fn optional_call_after_private_member_chain_lowers() {
    lower_src(
        r#"
        class Example {
          #hooks = { done() { return "done"; } };
          run() {
            return this.#hooks?.done?.();
          }
        }
        "#,
    )
    .expect("this.#hooks?.done?.() should lower");
}
