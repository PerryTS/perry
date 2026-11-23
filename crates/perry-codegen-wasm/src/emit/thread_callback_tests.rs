//! `perry/thread` callbacks on the web target.

use super::run_web_source;

// A thread callback that declares more parameters than it is passed receives undefined
// in the rest, as every other table call does (#15140).
#[test]
fn a_thread_callback_with_unpassed_parameters_receives_undefined() {
    assert_eq!(
        run_web_source(
            r#"
import { parallelFilter, parallelMap, spawn } from "perry/thread";
const k = 1;
console.log(parallelMap([1, 2], (n: number, m?: number) => n + k + ":" + m).join(","));
console.log(parallelFilter([1, 2], (n: number, m?: number) => m === undefined && n > k).join(","));
spawn((m?: number) => "spawn " + m).then((v: string) => console.log(v));
"#
        ),
        "2:undefined,3:undefined\n2\nspawn undefined"
    );
}
