use crate::value::{TAG_FALSE, TAG_TRUE};
use std::cell::Cell;

const IPC_CHANNEL_ENV: &str = "NODE_CHANNEL_FD";

crate::perry_thread_local! {
    static WORKER_IPC_ENABLED: Cell<bool> = const { Cell::new(false) };
}

// IPC members are fixed at Worker bootstrap, not by later process.env edits.
pub struct WorkerProcessState {
    ipc_enabled: bool,
    argv: Option<(Option<String>, Option<crate::thread::SerializedValue>)>,
}

impl WorkerProcessState {
    pub fn capture(env: Option<&[(String, String)]>) -> Self {
        let ipc_enabled = match env {
            Some(env) => env
                .iter()
                .find(|(key, _)| key == IPC_CHANNEL_ENV)
                .is_some_and(|(_, value)| !value.is_empty()),
            None => std::env::var_os(IPC_CHANNEL_ENV).is_some_and(|value| !value.is_empty()),
        };
        Self {
            ipc_enabled,
            argv: None,
        }
    }

    pub fn with_worker_argv(
        mut self,
        entry: Option<String>,
        argv: Option<crate::thread::SerializedValue>,
    ) -> Self {
        if entry.is_some() || argv.is_some() {
            self.argv = Some((entry, argv));
        }
        self
    }

    pub fn serialize_argv(value: f64) -> Result<crate::thread::SerializedValue, String> {
        let scope = crate::gc::RuntimeHandleScope::new();
        let value = scope.root_nanbox_f64(value);
        let len = crate::array::arraylike_length(value.get_nanbox_f64());
        let array = scope.root_raw_mut_ptr(crate::array::js_array_alloc(len as u32));
        for index in 0..len {
            let item = crate::array::arraylike_get(value.get_nanbox_f64(), index);
            crate::array::js_array_push_f64(array.get_raw_mut_ptr(), item);
        }
        unsafe {
            crate::thread::serialize_message(
                crate::value::JSValue::pointer(array.get_raw_const_ptr::<u8>()).bits(),
                &[],
                None,
            )
        }
    }

    pub fn install(self) {
        WORKER_IPC_ENABLED.with(|slot| slot.set(self.ipc_enabled));
        let Some((entry, argv)) = self.argv else {
            return;
        };
        crate::gc::gc_init();
        let scope = crate::gc::RuntimeHandleScope::new();
        let defaults = scope.root_raw_mut_ptr(crate::os::js_process_argv());
        let argv = scope.root_nanbox_f64(match argv {
            Some(value) => f64::from_bits(unsafe {
                crate::thread::deserialize_nanbox_on_current_thread(&value)
            }),
            None => f64::from_bits(crate::value::TAG_UNDEFINED),
        });
        let len = if argv.get_nanbox_f64().to_bits() == crate::value::TAG_UNDEFINED {
            0
        } else {
            crate::array::arraylike_length(argv.get_nanbox_f64())
        };
        let array = scope.root_raw_mut_ptr(crate::os::allocate_process_argv(
            (len as u32).saturating_add(2),
        ));
        crate::array::js_array_push_f64(
            array.get_raw_mut_ptr(),
            crate::array::js_array_get_f64(defaults.get_raw_mut_ptr(), 0),
        );
        let entry = match entry {
            Some(entry) => crate::value::js_nanbox_string(crate::string::js_string_from_bytes(
                entry.as_ptr(),
                entry.len() as u32,
            ) as i64),
            None => crate::array::js_array_get_f64(defaults.get_raw_mut_ptr(), 1),
        };
        crate::array::js_array_push_f64(array.get_raw_mut_ptr(), entry);
        for index in 0..len {
            let item = crate::array::arraylike_get(argv.get_nanbox_f64(), index);
            crate::array::js_array_push_f64(array.get_raw_mut_ptr(), item);
        }
        crate::object::native_namespace_prop_override_store(
            "process",
            "argv",
            crate::value::js_nanbox_pointer(
                array.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64
            ),
        );
    }
}

fn is_worker_ipc_member(name: &str) -> bool {
    matches!(
        worker_process_restriction(name),
        Some(Restriction::IpcCallable | Restriction::IpcGetter)
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Restriction {
    Disabled,
    ReadOnly,
    IpcCallable,
    IpcGetter,
    Hidden,
}

fn worker_process_restriction(name: &str) -> Option<Restriction> {
    Some(match name {
        "abort" | "chdir" | "setuid" | "seteuid" | "setgid" | "setegid" | "setgroups"
        | "initgroups" => Restriction::Disabled,
        "umask" => Restriction::ReadOnly,
        "send" | "disconnect" => Restriction::IpcCallable,
        "channel" | "connected" => Restriction::IpcGetter,
        "_startProfilerIdleNotifier"
        | "_stopProfilerIdleNotifier"
        | "_debugProcess"
        | "_debugPause"
        | "_debugEnd" => Restriction::Hidden,
        _ => return None,
    })
}

pub(crate) fn in_node_worker() -> bool {
    // Namespace properties are mutable; only the registered native getter
    // proves Worker identity independently of user overrides.
    crate::object::call_worker_threads_getter(
        &crate::object::WORKER_THREADS_IS_MAIN_THREAD_GETTER,
        || f64::from_bits(TAG_TRUE),
    )
    .to_bits()
        == TAG_FALSE
}

fn is_process_namespace(module: &str) -> bool {
    matches!(module, "process" | "process.namespace" | "process.default")
}

pub(crate) fn worker_process_callable_disabled(module: &str, name: &str) -> bool {
    is_process_namespace(module)
        && matches!(
            worker_process_restriction(name),
            Some(Restriction::Disabled | Restriction::IpcCallable)
        )
        && in_node_worker()
}

pub(crate) fn worker_process_member_hidden(module: &str, name: &str) -> bool {
    is_process_namespace(module)
        && (worker_process_restriction(name) == Some(Restriction::Hidden)
            || (is_worker_ipc_member(name) && !WORKER_IPC_ENABLED.with(Cell::get)))
        && in_node_worker()
        && crate::object::native_namespace_prop_override_get(module, name).is_none()
}

pub(crate) fn ensure_worker_operation_allowed(name: &str) {
    let Some(restriction) = worker_process_restriction(name) else {
        return;
    };
    if !in_node_worker() {
        return;
    }
    if restriction == Restriction::Hidden
        || (is_worker_ipc_member(name) && !WORKER_IPC_ENABLED.with(Cell::get))
    {
        crate::collection_iter::throw_type_error(&format!("process.{name} is not a function"));
    }
    crate::fs::validate::throw_type_error_with_code(
        &format!("process.{name}() is not supported in workers"),
        "ERR_WORKER_UNSUPPORTED_OPERATION",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_ipc_bootstrap_captures_explicit_environment() {
        assert!(!WorkerProcessState::capture(Some(&[])).ipc_enabled);
        let env = vec![("NODE_CHANNEL_FD".to_string(), String::new())];
        assert!(!WorkerProcessState::capture(Some(&env)).ipc_enabled);
        let env = vec![("NODE_CHANNEL_FD".to_string(), "worker-parity".to_string())];
        assert!(WorkerProcessState::capture(Some(&env)).ipc_enabled);
    }

    #[test]
    fn worker_member_policy_keeps_readers_and_writers_distinct() {
        for name in [
            "abort",
            "chdir",
            "setuid",
            "seteuid",
            "setgid",
            "setegid",
            "setgroups",
            "initgroups",
        ] {
            assert_eq!(
                worker_process_restriction(name),
                Some(Restriction::Disabled)
            );
        }
        assert_eq!(
            worker_process_restriction("send"),
            Some(Restriction::IpcCallable)
        );
        assert_eq!(
            worker_process_restriction("umask"),
            Some(Restriction::ReadOnly)
        );
        assert_eq!(
            worker_process_restriction("connected"),
            Some(Restriction::IpcGetter)
        );
        assert_eq!(
            worker_process_restriction("_debugProcess"),
            Some(Restriction::Hidden)
        );
        assert_eq!(worker_process_restriction("cwd"), None);
    }
}
