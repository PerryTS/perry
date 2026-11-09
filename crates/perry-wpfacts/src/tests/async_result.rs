//! An async function without `await` runs untransformed: `async_to_generator`
//! rewrites only bodies with an await, and codegen lowers the rest direct
//! (`lower_async_rejecting_stmts`). A call returns a fresh promise settled
//! with what the body returns (a returned thenable adopted) and rejected
//! with what the body throws; it never throws itself. The facts model that
//! promise, not ⊤ (skin seed: `Promise.all(p).then(async function (r) {..})`
//! in GLTFLoader, ASYNCRES).

use super::*;

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

fn program(body: &str) -> String {
    format!(
        r#"
        class O {{ ax: number; constructor(ax: number) {{ this.ax = ax; }} }}
        const o = new O(1);
        {body}
        "#
    )
}

/// Not from unmodeled code. Adoption still passes a promise or thenable
/// itself on beside its settled value (`builtins::adopt`, the value edge),
/// so a reaction's receiver set may hold the promise too: these sites stay
/// unproven for that reason, never for ⊤.
fn known(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(r) if r.contains("unmodeled code"))
}

/// The verdict of the one `ax` read.
fn ax(body: &str) -> Verdict {
    site(&facts(&program(body)), "ax").verdict.clone()
}

/// node: prints `1`.
#[test]
fn an_async_function_result_settles_with_its_return() {
    let v = ax(r#"
        async function af(): Promise<any> { return o; }
        af().then((r: any) => console.log(r.ax));
    "#);
    assert!(proven(&v), "{v:?}");
}

/// node: prints `1`.
#[test]
fn an_async_arrow_result_settles_with_its_return() {
    let v = ax(r#"
        const aa = async () => o;
        aa().then((r: any) => console.log(r.ax));
    "#);
    assert!(proven(&v), "{v:?}");
}

/// node: prints `1`.
#[test]
fn an_async_method_result_settles_with_its_return() {
    let v = ax(r#"
        class K { async m(): Promise<any> { return o; } }
        new K().m().then((r: any) => console.log(r.ax));
    "#);
    assert!(proven(&v), "{v:?}");
}

/// The skin shape (GLTFLoader loadMesh). node: prints `1`.
#[test]
fn an_async_then_callback_result_is_its_promise() {
    let v = ax(r#"
        const pending: any[] = [Promise.resolve(0)];
        Promise.all(pending)
            .then(async function (results: any) { return o; })
            .then((r: any) => console.log(r.ax));
    "#);
    assert!(known(&v), "{v:?}");
}

/// node: prints `1`.
#[test]
fn async_map_callbacks_give_promises() {
    let v = ax(r#"
        const ps = [1].map(async (x: number) => o);
        ps[0].then((r: any) => console.log(r.ax));
    "#);
    assert!(proven(&v), "{v:?}");
}

/// A returned promise is adopted, not passed through. node: prints `1`.
#[test]
fn a_returned_promise_is_adopted() {
    let v = ax(r#"
        async function af(): Promise<any> { return Promise.resolve(o); }
        af().then((r: any) => console.log(r.ax));
    "#);
    assert!(known(&v), "{v:?}");
}

/// A returned thenable is adopted through its `then`. node: prints `1`.
#[test]
fn a_returned_thenable_is_adopted() {
    let v = ax(r#"
        const th = { then(res: any, rej: any) { res(o); } };
        async function af(): Promise<any> { return th; }
        af().then((r: any) => console.log(r.ax));
    "#);
    assert!(known(&v), "{v:?}");
}

/// Control: the settled value is what the body returns, so an unknown
/// return reaches the reaction (the promise is not an empty claim).
#[test]
fn an_unknown_return_reaches_the_reaction() {
    let v = ax(r#"
        async function af(): Promise<any> { return (globalThis as any).unknownHostValue; }
        af().then((r: any) => console.log(r.ax));
    "#);
    assert!(!proven(&v), "{v:?}");
}

/// Control: an unknown thenable's `then` resolves with anything.
#[test]
fn an_unknown_thenable_reaches_the_reaction() {
    let v = ax(r#"
        const aa = async () => (globalThis as any).unknownHostValue;
        aa().then((r: any) => console.log(r.ax));
    "#);
    assert!(!proven(&v), "{v:?}");
}

/// What the body throws rejects the promise: an unknown thrown value
/// reaches `catch`. node: the rejection handler gets the thrown value.
#[test]
fn a_throw_rejects_the_promise() {
    let v = ax(r#"
        async function af(): Promise<any> { throw (globalThis as any).unknownHostValue; }
        af().catch((e: any) => console.log(e.ax));
    "#);
    assert!(!proven(&v), "{v:?}");
}
