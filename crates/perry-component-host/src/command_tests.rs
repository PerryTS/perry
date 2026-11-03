use super::*;

fn component(body: &str) -> Vec<u8> {
    wat::parse_str(format!(
        r#"(component
          (core module $code
            (func (export "run") (result i32) {body}))
          (core instance $code (instantiate $code))
          (func $run (result (result))
            (canon lift (core func $code "run")))
          (instance $run (export "run" (func $run)))
          (export "wasi:cli/run@0.2.0" (instance $run)))"#
    ))
    .unwrap()
}

fn exit_component(code: u8, with_code: bool, start: bool, export: bool) -> Vec<u8> {
    wat::parse_str(exit_source(code, with_code, start, export)).unwrap()
}

pub(super) fn exit_source(code: u8, with_code: bool, start: bool, export: bool) -> String {
    let (name, param, ty, version) = if with_code {
        ("exit-with-code", "status-code", "u8", "0.2.12")
    } else {
        ("exit", "status", "(result)", "0.2.0")
    };
    let call = format!("(call $exit (i32.const {code})) unreachable");
    let run = if start { "i32.const 0" } else { &call };
    let start = if start {
        format!("(func $start {call}) (start $start)")
    } else {
        String::new()
    };
    let export = if export {
        r#"(export "wasi:cli/run@0.2.0" (instance $run))"#
    } else {
        ""
    };
    format!(
        r#"(component
          (import "wasi:cli/exit@{version}" (instance $exit
            (export "{name}" (func (param "{param}" {ty})))))
          (alias export $exit "{name}" (func $exit))
          (core func $exit (canon lower (func $exit)))
          (core instance $env (export "exit" (func $exit)))
          (core module $code
            (import "env" "exit" (func $exit (param i32)))
            {start}
            (func (export "run") (result i32) {run}))
          (core instance $code (instantiate $code (with "env" (instance $env))))
          (func $run (result (result))
            (canon lift (core func $code "run")))
          (instance $run (export "run" (func $run)))
          {export})"#
    )
}

#[test]
fn return_and_exit_statuses_do_not_terminate_the_host() {
    for status in [0, 1] {
        assert_eq!(
            run_command(
                &component(&format!("i32.const {status}")),
                &Options::default()
            )
            .unwrap(),
            status
        );
        // The unreachable instruction immediately after exit must never run.
        assert_eq!(
            run_command(
                &exit_component(status as u8, false, false, true),
                &Options::default()
            )
            .unwrap(),
            status
        );
    }
    for status in [0, 23, 255] {
        assert_eq!(
            run_command(
                &exit_component(status, true, false, true),
                &Options::default()
            )
            .unwrap(),
            status as i32
        );
    }
    assert_eq!(
        run_command(&component("i32.const 0"), &Options::default()).unwrap(),
        0
    );
}

#[test]
fn initialization_exit_requires_a_valid_command_export() {
    assert_eq!(
        run_command(&exit_component(23, true, true, true), &Options::default()).unwrap(),
        23
    );
    let error =
        run_command(&exit_component(0, false, true, false), &Options::default()).unwrap_err();
    assert!(format!("{error:#}").contains("validating wasi:cli/run"));

    // An init-time exit(0) must not hide a wrong run signature. CommandPre's
    // name-only lookup alone would accept the export until after instantiation.
    let source = exit_source(0, false, true, true).replace("(result (result))", "(result u32)");
    let bytes = wat::parse_str(source).unwrap();
    let error = run_command(&bytes, &Options::default()).unwrap_err();
    assert!(format!("{error:#}").contains("must return result without payloads"));
}

#[test]
fn traps_bad_results_and_invalid_components_are_not_exit_statuses() {
    for bytes in [
        component("unreachable"),
        component("i32.const 2"),
        vec![0, 1, 2],
    ] {
        assert!(run_command(&bytes, &Options::default()).is_err());
    }
    let bytes = wat::parse_str("(component)").unwrap();
    assert!(run_command(&bytes, &Options::default()).is_err());
}

#[test]
fn command_fuel_and_memory_limits_are_enforced() {
    let options = Options {
        fuel: 100,
        ..Options::default()
    };
    let error = run_command(
        &component("(loop $forever (br $forever)) unreachable"),
        &options,
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("fuel"));
    let bytes = component("i32.const 0");
    assert!(run_command(&bytes, &Options { fuel: 0, ..options }).is_err());
    let source = r#"(component
      (core module $code
        (memory 2)
        (func (export "run") (result i32) i32.const 0))
      (core instance $code (instantiate $code))
      (func $run (result (result)) (canon lift (core func $code "run")))
      (instance $run (export "run" (func $run)))
      (export "wasi:cli/run@0.2.0" (instance $run)))"#;
    let bytes = wat::parse_str(source).unwrap();
    let error = run_command(
        &bytes,
        &Options {
            memory_bytes: 65536,
            ..Options::default()
        },
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("memory"));
}

#[test]
fn ffi_distinguishes_guest_status_from_embedding_failure() {
    unsafe {
        for (bytes, expected, value) in [
            (
                exit_component(23, true, false, true),
                perry_abi::component_host::OK,
                Some(23),
            ),
            (vec![0, 1, 2], perry_abi::component_host::ERROR, None),
        ] {
            let mut output = perry_abi::component_host::Buffer::default();
            let status = crate::ffi::perry_component_host_run_command(
                bytes.as_ptr(),
                bytes.len(),
                std::ptr::null(),
                0,
                &mut output,
            );
            assert_eq!(status, expected);
            let data = std::slice::from_raw_parts(output.data, output.len);
            if let Some(value) = value {
                assert_eq!(serde_json::from_slice::<i32>(data).unwrap(), value);
            } else {
                assert!(!data.is_empty());
            }
            crate::ffi::perry_component_host_buffer_drop(output);
        }
    }
}
