//! Exercise canonical imports and exports of the same interface with a Rust oracle.
use super::State;
use anyhow::{ensure, Context, Result};
use wasmtime::component::{Instance, Linker, Val};
use wasmtime::Store;

const INTERFACE: &str = "perry:all-values/values@1.0.0";
const CALLS: usize = 12;

fn input(n: usize) -> Val {
    let alternate = n % 2 != 0;
    let choice = match n % 3 {
        0 => Val::Variant("absent".into(), None),
        1 => Val::Variant("text".into(), Some(Box::new(Val::String("é\0🌍".into())))),
        _ => Val::Variant(
            "colors".into(),
            Some(Box::new(Val::List(vec![
                Val::Enum("red".into()),
                Val::Enum("green".into()),
                Val::Enum("blue".into()),
            ]))),
        ),
    };
    let nested = match n % 3 {
        0 => Val::Option(None),
        1 => Val::Option(Some(Box::new(Val::Option(None)))),
        _ => Val::Option(Some(Box::new(Val::Option(Some(Box::new(Val::String(
            "nested 🌍".into(),
        ))))))),
    };
    Val::Record(
        vec![
            ("a", Val::S8(if alternate { i8::MAX } else { i8::MIN })),
            ("b", Val::U8(if alternate { u8::MAX } else { 0 })),
            ("c", Val::S16(if alternate { i16::MAX } else { i16::MIN })),
            ("d", Val::U16(if alternate { u16::MAX } else { 0 })),
            ("e", Val::S32(if alternate { i32::MAX } else { i32::MIN })),
            ("f", Val::U32(if alternate { u32::MAX } else { 0 })),
            ("g", Val::S64(if alternate { i64::MAX } else { i64::MIN })),
            ("h", Val::U64(if alternate { u64::MAX } else { 0 })),
            (
                "i",
                Val::Float32(if alternate { -1.25 } else { f32::MIN_POSITIVE }),
            ),
            (
                "j",
                Val::Float64(if alternate {
                    f64::MAX
                } else {
                    f64::MIN_POSITIVE
                }),
            ),
            ("k", Val::Char(if alternate { '🌍' } else { '\0' })),
            ("l", Val::Bool(alternate)),
            (
                "text",
                Val::String(if alternate { "héllo\0世界🌍" } else { "" }.into()),
            ),
            (
                "bytes",
                Val::List(if alternate {
                    vec![Val::U8(0), Val::U8(127), Val::U8(255)]
                } else {
                    vec![]
                }),
            ),
            (
                "permissions",
                Val::Flags(if alternate {
                    vec![
                        "bit0".into(),
                        "bit16".into(),
                        "bit30".into(),
                        "bit31".into(),
                    ]
                } else {
                    vec![]
                }),
            ),
            ("choice", choice),
            ("nested", nested),
            (
                "pair",
                Val::Tuple(vec![
                    Val::String("tuple 🌍".into()),
                    Val::List(vec![Val::U16(0), Val::U16(65535)]),
                ]),
            ),
        ]
        .into_iter()
        .map(|(name, value)| (name.into(), value))
        .collect(),
    )
}

fn output(n: usize) -> Val {
    if n % 4 == 3 {
        Val::Result(Err(Some(Box::new(Val::Variant(
            "text".into(),
            Some(Box::new(Val::String("typed error 🌍".into()))),
        )))))
    } else {
        Val::Result(Ok(Some(Box::new(input(n)))))
    }
}

pub(super) fn link(linker: &mut Linker<State>) -> Result<()> {
    let mut api = linker.instance(INTERFACE)?;
    api.func_new("echo", |mut store, _, args, results| {
        let n = store.data().logs.len();
        wasmtime::ensure!(
            args == [input(n)],
            "canonical import altered case {n}: {args:?}"
        );
        results[0] = output(n);
        store.data_mut().logs.push("echo".into());
        Ok(())
    })?;
    api.func_wrap(
        "notify",
        |mut store: wasmtime::StoreContextMut<'_, State>, (text,): (String,)| {
            wasmtime::ensure!(text == "void é\0🌍", "void import lost Unicode/NUL");
            store.data_mut().logs.push("notify".into());
            Ok(())
        },
    )?;
    api.func_wrap(
        "scalar",
        |mut store: wasmtime::StoreContextMut<'_, State>, (): ()| {
            store.data_mut().logs.push("scalar".into());
            Ok((u64::MAX,))
        },
    )?;
    Ok(())
}

pub(super) fn run(store: &mut Store<State>, instance: &Instance) -> Result<()> {
    let interface = instance
        .get_export_index(&mut *store, None, INTERFACE)
        .context("missing values interface")?;
    let index = instance
        .get_export_index(&mut *store, Some(&interface), "echo")
        .context("missing echo")?;
    let echo = instance
        .get_func(&mut *store, &index)
        .context("missing echo function")?;
    for n in 0..CALLS {
        store.set_fuel(100_000_000)?;
        let mut result = [Val::Bool(false)];
        echo.call(&mut *store, &[input(n)], &mut result)?;
        ensure!(
            result == [output(n)],
            "canonical export altered case {n}: {result:?}"
        );
    }
    let index = instance
        .get_export_index(&mut *store, Some(&interface), "notify")
        .context("missing notify")?;
    let notify = instance
        .get_func(&mut *store, &index)
        .context("missing notify function")?
        .typed::<(String,), ()>(&*store)?;
    notify.call(&mut *store, ("void é\0🌍".into(),))?;
    let index = instance
        .get_export_index(&mut *store, Some(&interface), "scalar")
        .context("missing scalar")?;
    let scalar = instance
        .get_func(&mut *store, &index)
        .context("missing scalar function")?
        .typed::<(), (u64,)>(&*store)?;
    ensure!(
        scalar.call(&mut *store, ())? == (u64::MAX,),
        "scalar u64 lost precision"
    );
    ensure!(
        store.data().logs.len() == CALLS + 2,
        "imports did not execute exactly once"
    );
    println!("PASS canonical values: 14 import/export calls, integer limits, BigInt, Unicode, flags32, nested options, variants, tuples, typed errors, guest GC");
    Ok(())
}
