//! Rule PK: `Object.getOwnPropertyNames` of a class prototype gives the
//! keys the class defines on it and those the program wrote on it
//! (PERFNOW43, ladder blocker c08-B3: a hook that wraps every prototype
//! method of a context stored the wrapper under any key, so every field of
//! the context, `TEX` among them, held the wrapper).
use super::*;

fn facts_of(src: &str) -> ProgramFacts {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let list: Vec<(String, &perry_hir::Module)> = vec![(path, &hir)];
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::compute(&list, &cfg)
}

/// Does `Ctx.TEX` hold the number only (no wrapper)?
fn tex_kept(extra: &str) -> (bool, String) {
    let src = format!(
        "class Core {{
          _errs: number[] = [];
          _synth(code: number): void {{ this._errs.push(code); }}
        }}
        class Ctx extends Core {{
          readonly TEX = 3553;
          getError(): number {{ return this._errs.length > 0 ? (this._errs.shift() as number) : 0; }}
          bind(t: number): void {{ if (t < 0) this._synth(1280); }}
        }}
        {extra}
        function hook(gl: Record<string, unknown>): void {{
          for (let p = Object.getPrototypeOf(gl); p !== null && p !== Object.prototype; p = Object.getPrototypeOf(p)) {{
            for (const k of Object.getOwnPropertyNames(p)) {{
              const d = Object.getOwnPropertyDescriptor(p, k);
              if (d === undefined || typeof d.value !== \"function\") continue;
              const f = d.value as (...a: unknown[]) => unknown;
              gl[k] = (...a: unknown[]): unknown => f.apply(gl, a);
            }}
          }}
        }}
        const gl = new Ctx();
        hook(gl as unknown as Record<string, unknown>);
        const t = gl.TEX;
        gl.bind(t + 1);
        console.log(t, gl.getError());"
    );
    let pf = facts_of(&src);
    let dump = pf.dump_text();
    // The kind of the field: the number only, or the wrapper too.
    let kept = dump
        .lines()
        .any(|l| l == "FACT F2 kind main.ts:Ctx.TEX = num");
    (kept, dump)
}

/// RED on 307741ba1d: the prototype's names were any String, so the hook
/// stored the wrapper under `TEX` too.
#[test]
fn pk_class_prototype_names() {
    let (ok, why) = tex_kept("");
    assert!(ok, "gl.TEX keeps its one field: {why}");
}

/// A store under a key no name lists on the prototype: its names are any
/// String again, and the hook may write `TEX`.
#[test]
fn pk_unlisted_key_store_widens() {
    let (ok, why) = tex_kept(
        "const s = String(Math.random());
        (Ctx.prototype as any)[s] = function (): number { return 1; };",
    );
    assert!(!ok, "gl.TEX may hold the wrapper: {why}");
}

/// A named store on the prototype adds that name, nothing else.
#[test]
fn pk_named_store_listed() {
    let (ok, why) = tex_kept("(Ctx.prototype as any).extra = function (): number { return 2; };");
    assert!(ok, "gl.TEX keeps its one field: {why}");
}
