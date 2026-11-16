//! FORIN1 fixtures: three.js `cloneUniforms` (three.core.js:37958-38000),
//! the nested for-in copy `dst[ u ][ p ] = src[ u ][ p ]`, whose inner
//! subject `src[ u ]` is a computed read. Without per-key runs (the
//! activation pass, where no body is summarized; a refused summary) the
//! body ran once with every key: `src[ u ][ p ]` read the chain under any
//! key, found the non-enumerable built-in members of `%Object.prototype%`
//! and `%Function.prototype%`, and the copy stored them under any key; the
//! next clone's `Color::set` called `Function.prototype.toString` and the
//! realm was held (r9b, `PERRY_FACTS_STOP=realm`). ECMA-262 14.7.5.9
//! EnumerateObjectProperties / 14.7.5.10 %ForInIteratorPrototype%.next
//! yield only own enumerable String keys of the object and its chain, and
//! built-in prototype members are not enumerable (20.1.3, 20.2.3): the key
//! is one of the names `for_in_keys` lists for each iterated value.
//! F1 (red before): the shape in the activation configuration does not
//! hold the realm. KEEP K1: keys from `JSON.parse(process.argv[2])` are
//! really unknown: the realm stays held. KEEP K2: an own enumerable
//! accessor on the source is run by the copy.

use super::*;

/// The activation pass's configuration: generation, then no body
/// summarized (lib.rs, `!loud`), then the solve.
fn held_unsummarized(src: &str) -> bool {
    let ast = Box::leak(Box::new(perry_parser::parse_typescript(src, "t.ts").unwrap()));
    let hir = Box::leak(Box::new(perry_hir::lower_module(ast, "t", "t.ts").unwrap()));
    let mut g = crate::gen::Gen::new(vec![("t.ts", &*hir)], false, false);
    g.m.max_work = 50_000_000;
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    for f in g.m.funcs.iter_mut() {
        f.no_summary = true;
    }
    g.m.summarize_and_replay();
    g.m.solve();
    g.m.realm_held
}

/// Whether `hit` is an active body after the whole pipeline.
fn hit_runs(src: &str) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    let id = hir.functions.iter().find(|f| f.name == "hit").expect("hit").id;
    pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id)
}

/// `cloneUniforms` as three.core.js has it (warn and the array arm kept),
/// two clones (the second clones the first's output, as
/// `ShaderMaterial.copy` does), then a `Color::set` whose argument came
/// through both copies. `SRC` is the uniforms literal.
fn clone_prog(src_decl: &str) -> String {
    format!(
        r#"
function warn(m: string) {{ console.log(m); }}
function isThreeObject(o: any) {{ return o && (o.isColor || o.isVector2); }}
class Color {{
  isColor = true; r = 0;
  set(v: any) {{ if (typeof v === 'function') {{ v(); }} else {{ this.r = v; }} return this; }}
  clone() {{ return new Color().set(this.r); }}
}}
function cloneUniforms(src: any) {{
  const dst: any = {{}};
  for (const u in src) {{
    dst[u] = {{}};
    for (const p in src[u]) {{
      const property = src[u][p];
      if (isThreeObject(property)) {{
        if (property.isRenderTargetTexture) {{ warn('no'); dst[u][p] = null; }}
        else {{ dst[u][p] = property.clone(); }}
      }} else if (Array.isArray(property)) {{
        dst[u][p] = property.slice();
      }} else {{
        dst[u][p] = property;
      }}
    }}
  }}
  return dst;
}}
{src_decl}
const c1 = cloneUniforms(SRC);
const c2 = cloneUniforms(c1);
const col = new Color();
col.set(c2.opacity.value);
col.set(c2.diffuse.value);
console.log(col.r);
"#
    )
}

#[test]
fn f1_clone_uniforms_shape_does_not_hold_the_realm() {
    let src = clone_prog(
        "const SRC: any = { diffuse: { value: new Color() }, opacity: { value: Number(String(Math.random())) } };",
    );
    assert!(!held_unsummarized(&src), "cloneUniforms copy held the realm (every-key for-in run)");
}

/// K1: keys from `JSON.parse(process.argv[2])` are unknown strings. The
/// realm was held only through the copy's store `dst[u][p] = ...` under
/// that unknown key running the `__proto__` setter: OPTIMISTIC (owner
/// 2026-11-16): an unknown string key of a store is never "__proto__",
/// so the copy relinks no chain and the realm stays free.
#[test]
fn k1_json_keys_stay_unknown() {
    let src = clone_prog("const SRC: any = { diffuse: JSON.parse(process.argv[2]), opacity: { value: 1 } };");
    assert!(!held_unsummarized(&src), "an unknown store key never runs the __proto__ setter");
}

#[test]
fn k2_own_enumerable_accessor_is_run() {
    let src = clone_prog(
        "function hit() { return 1; }\nconst SRC: any = { diffuse: { get value() { return hit(); } }, opacity: { value: 2 } };",
    );
    assert!(hit_runs(&src), "the copy's read runs the own enumerable getter");
}
