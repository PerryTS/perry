use super::{bindings::wasi::cli::*, Exit, State, TerminalInput, TerminalOutput};
use std::io::IsTerminal;
use wasmtime::component::Resource;

impl environment::Host for State {
    fn get_environment(&mut self) -> wasmtime::Result<Vec<(String, String)>> {
        Ok(self
            .options
            .env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }
    fn get_arguments(&mut self) -> wasmtime::Result<Vec<String>> {
        Ok(self.options.args.clone())
    }
    fn initial_cwd(&mut self) -> wasmtime::Result<Option<String>> {
        Ok(None)
    }
}

impl exit::Host for State {
    fn exit(&mut self, status: Result<(), ()>) -> wasmtime::Result<()> {
        Err(wasmtime::format_err!(Exit(i32::from(status.is_err()))))
    }
    fn exit_with_code(&mut self, code: u8) -> wasmtime::Result<()> {
        Err(wasmtime::format_err!(Exit(i32::from(code))))
    }
}

impl terminal_input::Host for State {}
impl terminal_input::HostTerminalInput for State {
    fn drop(&mut self, terminal: Resource<TerminalInput>) -> wasmtime::Result<()> {
        self.table.delete(terminal)?;
        Ok(())
    }
}
impl terminal_output::Host for State {}
impl terminal_output::HostTerminalOutput for State {
    fn drop(&mut self, terminal: Resource<TerminalOutput>) -> wasmtime::Result<()> {
        self.table.delete(terminal)?;
        Ok(())
    }
}
impl terminal_stdin::Host for State {
    fn get_terminal_stdin(&mut self) -> wasmtime::Result<Option<Resource<TerminalInput>>> {
        if self.options.inherit_stdin && std::io::stdin().is_terminal() {
            Ok(Some(self.table.push(TerminalInput)?))
        } else {
            Ok(None)
        }
    }
}
impl terminal_stdout::Host for State {
    fn get_terminal_stdout(&mut self) -> wasmtime::Result<Option<Resource<TerminalOutput>>> {
        if self.options.inherit_stdout && std::io::stdout().is_terminal() {
            Ok(Some(self.table.push(TerminalOutput)?))
        } else {
            Ok(None)
        }
    }
}
impl terminal_stderr::Host for State {
    fn get_terminal_stderr(&mut self) -> wasmtime::Result<Option<Resource<TerminalOutput>>> {
        if self.options.inherit_stderr && std::io::stderr().is_terminal() {
            Ok(Some(self.table.push(TerminalOutput)?))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WasiOptions;

    #[test]
    fn defaults_expose_no_environment_directory_or_terminals() {
        let mut state = State::new(&WasiOptions::default()).unwrap();
        assert!(environment::Host::get_arguments(&mut state)
            .unwrap()
            .is_empty());
        assert!(environment::Host::get_environment(&mut state)
            .unwrap()
            .is_empty());
        assert!(environment::Host::initial_cwd(&mut state)
            .unwrap()
            .is_none());
        assert!(terminal_stdin::Host::get_terminal_stdin(&mut state)
            .unwrap()
            .is_none());
        assert!(terminal_stdout::Host::get_terminal_stdout(&mut state)
            .unwrap()
            .is_none());
        assert!(terminal_stderr::Host::get_terminal_stderr(&mut state)
            .unwrap()
            .is_none());
    }

    #[test]
    fn exits_are_typed_guest_errors_with_exact_statuses() {
        let mut state = State::new(&WasiOptions::default()).unwrap();
        for code in [0, 1, 127, 255] {
            let error = exit::Host::exit_with_code(&mut state, code).unwrap_err();
            assert_eq!(error.downcast_ref::<Exit>().unwrap().0, i32::from(code));
        }
        for (status, code) in [(Ok(()), 0), (Err(()), 1)] {
            let error = exit::Host::exit(&mut state, status).unwrap_err();
            assert_eq!(error.downcast_ref::<Exit>().unwrap().0, code);
        }
    }
}
