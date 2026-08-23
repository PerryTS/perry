//! Deferred `node:test` registration, scoped hooks, subtests, and reporting.
//!
//! Node registers tests while a module is evaluated and starts the root suite
//! afterwards. Keeping that boundary is important: it is what makes top-level
//! hooks cover every test, lets `.only` filter siblings, and produces one
//! aggregate reporter summary instead of one summary per registration call.

use super::*;

#[derive(Clone, Copy)]
enum RegisteredNode {
    Test(usize),
    Suite(usize),
}

#[derive(Clone)]
struct TestDef {
    name: String,
    mode: TestMode,
    reason: Option<String>,
    callback: f64,
}

#[derive(Clone)]
struct SuiteDef {
    name: String,
    mode: TestMode,
    reason: Option<String>,
    callback: f64,
    before: Vec<f64>,
    after: Vec<f64>,
    before_each: Vec<f64>,
    after_each: Vec<f64>,
    children: Vec<RegisteredNode>,
}

impl SuiteDef {
    fn root() -> Self {
        Self {
            name: "<root>".to_string(),
            mode: TestMode::Normal,
            reason: None,
            callback: undefined_value(),
            before: Vec::new(),
            after: Vec::new(),
            before_each: Vec::new(),
            after_each: Vec::new(),
            children: Vec::new(),
        }
    }
}

struct RunnerState {
    scheduled: bool,
    running: bool,
    current_suite: usize,
    suites: Vec<SuiteDef>,
    tests: Vec<TestDef>,
}

impl RunnerState {
    fn new() -> Self {
        Self {
            scheduled: false,
            running: false,
            current_suite: 0,
            suites: vec![SuiteDef::root()],
            tests: Vec::new(),
        }
    }
}

#[derive(Clone)]
struct TestResult {
    name: String,
    mode: TestMode,
    reason: Option<String>,
    diagnostics: Vec<String>,
    failed: bool,
    pending: Option<f64>,
    children: Vec<TestResult>,
    suite: bool,
}

#[derive(Default)]
struct Stats {
    tests: u32,
    suites: u32,
    passed: u32,
    failed: u32,
    skipped: u32,
    todo: u32,
}

#[derive(Clone, Copy)]
enum HookKind {
    Before,
    After,
    BeforeEach,
    AfterEach,
}

thread_local! {
    static TEST_RUNNER: RefCell<RunnerState> = RefCell::new(RunnerState::new());
    static ACTIVE_CHILDREN: RefCell<Vec<Vec<TestResult>>> = const { RefCell::new(Vec::new()) };
    static ACTIVE_ANCESTORS: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    static DONE_CALLED: Cell<bool> = const { Cell::new(false) };
}

fn ensure_runner_scheduled() {
    let should_schedule = TEST_RUNNER.with(|runner| {
        let mut runner = runner.borrow_mut();
        if runner.scheduled || runner.running {
            false
        } else {
            runner.scheduled = true;
            true
        }
    });
    if should_schedule {
        let closure = make_closure(test_runner_task as *const u8, 0, 0);
        crate::timer::js_set_immediate_callback(closure as i64);
    }
}

fn option_reason(options: f64, name: &[u8]) -> Option<Option<String>> {
    let value = object_property(options, name)?;
    if crate::value::js_is_truthy(value) == 0 {
        return None;
    }
    Some(value_to_string(value))
}

fn validate_registration_options(options: f64) {
    if is_undefined_value(options) {
        return;
    }
    if !is_non_null_object(options) {
        throw_invalid_arg_type("options", "object", options);
    }
    if let Some(timeout) = object_property(options, b"timeout") {
        if !is_undefined_value(timeout) {
            crate::validators::validate_number(timeout, "options.timeout", Some(0.0), None);
        }
    }
    if let Some(concurrency) = object_property(options, b"concurrency") {
        let js = JSValue::from_bits(concurrency.to_bits());
        if !is_undefined_value(concurrency) && !js.is_bool() {
            crate::validators::validate_integer(
                concurrency,
                "options.concurrency",
                1.0,
                crate::validators::MAX_SAFE_INTEGER,
            );
        }
    }
}

fn callback_name(callback: f64) -> String {
    let name = mock_function_metadata(callback).0;
    if name.is_empty() {
        "<anonymous>".to_string()
    } else {
        name
    }
}

fn normalize_registration(
    base_mode: TestMode,
    name_or_callback: f64,
    options_or_callback: f64,
    callback: f64,
) -> TestDef {
    let (name, options, callback) = if is_callable_value(name_or_callback) {
        (
            callback_name(name_or_callback),
            undefined_value(),
            name_or_callback,
        )
    } else if is_non_null_object(name_or_callback) {
        let callback = if is_callable_value(options_or_callback) {
            options_or_callback
        } else {
            callback
        };
        let name = if is_callable_value(callback) {
            callback_name(callback)
        } else {
            "test".to_string()
        };
        (name, name_or_callback, callback)
    } else if is_callable_value(options_or_callback) {
        (
            value_to_string(name_or_callback).unwrap_or_else(|| "test".to_string()),
            undefined_value(),
            options_or_callback,
        )
    } else {
        (
            value_to_string(name_or_callback).unwrap_or_else(|| "test".to_string()),
            options_or_callback,
            callback,
        )
    };

    validate_registration_options(options);
    let skip = option_reason(options, b"skip");
    let todo = option_reason(options, b"todo");
    let (mode, reason) = if base_mode == TestMode::Skip || skip.is_some() {
        (TestMode::Skip, skip.flatten())
    } else if base_mode == TestMode::Todo || todo.is_some() {
        (TestMode::Todo, todo.flatten())
    } else {
        (base_mode, None)
    };
    TestDef {
        name,
        mode,
        reason,
        callback,
    }
}

fn register_test(
    mode: TestMode,
    name_or_callback: f64,
    options_or_callback: f64,
    callback: f64,
) -> f64 {
    ensure_runner_scheduled();
    let def = normalize_registration(mode, name_or_callback, options_or_callback, callback);

    if CURRENT_TEST_NAME.with(|name| name.borrow().is_some()) {
        CURRENT_ASSERT_COUNT.with(|count| count.set(count.get() + 1));
        let result = execute_inline_subtest(def);
        let returned = result.pending.unwrap_or_else(undefined_value);
        ACTIVE_CHILDREN.with(|children| {
            if let Some(frame) = children.borrow_mut().last_mut() {
                frame.push(result);
            }
        });
        if is_undefined_value(returned) {
            boxed_ptr(crate::promise::js_promise_resolved(undefined_value()))
        } else {
            returned
        }
    } else {
        TEST_RUNNER.with(|runner| {
            let mut runner = runner.borrow_mut();
            let id = runner.tests.len();
            runner.tests.push(def);
            let current = runner.current_suite;
            runner.suites[current]
                .children
                .push(RegisteredNode::Test(id));
        });
        undefined_value()
    }
}

fn register_suite(
    mode: TestMode,
    name_or_callback: f64,
    options_or_callback: f64,
    callback: f64,
) -> f64 {
    ensure_runner_scheduled();
    let def = normalize_registration(mode, name_or_callback, options_or_callback, callback);
    TEST_RUNNER.with(|runner| {
        let mut runner = runner.borrow_mut();
        let id = runner.suites.len();
        runner.suites.push(SuiteDef {
            name: def.name,
            mode: def.mode,
            reason: def.reason,
            callback: def.callback,
            before: Vec::new(),
            after: Vec::new(),
            before_each: Vec::new(),
            after_each: Vec::new(),
            children: Vec::new(),
        });
        let current = runner.current_suite;
        runner.suites[current]
            .children
            .push(RegisteredNode::Suite(id));
    });
    undefined_value()
}

fn register_hook(kind: HookKind, callback: f64) -> f64 {
    ensure_runner_scheduled();
    assert_callable_arg("fn", callback);
    TEST_RUNNER.with(|runner| {
        let mut runner = runner.borrow_mut();
        let current = runner.current_suite;
        let hooks = match kind {
            HookKind::Before => &mut runner.suites[current].before,
            HookKind::After => &mut runner.suites[current].after,
            HookKind::BeforeEach => &mut runner.suites[current].before_each,
            HookKind::AfterEach => &mut runner.suites[current].after_each,
        };
        hooks.push(callback);
    });
    undefined_value()
}

struct ContextSnapshot {
    name: Option<String>,
    diagnostics: Vec<String>,
    snapshot_index: u32,
    assertion_count: u32,
    plan: Option<u32>,
    directive: i8,
}

fn enter_context(name: &str) -> ContextSnapshot {
    let old_name = CURRENT_TEST_NAME.with(|slot| slot.borrow_mut().replace(name.to_string()));
    let diagnostics = CURRENT_DIAGNOSTICS.with(|slot| std::mem::take(&mut *slot.borrow_mut()));
    let snapshot_index = CURRENT_SNAPSHOT_INDEX.with(|slot| slot.replace(0));
    let assertion_count = CURRENT_ASSERT_COUNT.with(|slot| slot.replace(0));
    let plan = CURRENT_PLAN.with(|slot| slot.replace(None));
    let directive = CURRENT_TEST_OVERRIDE.with(|slot| slot.replace(TEST_OVERRIDE_NONE));
    ContextSnapshot {
        name: old_name,
        diagnostics,
        snapshot_index,
        assertion_count,
        plan,
        directive,
    }
}

fn restore_context(snapshot: ContextSnapshot) {
    CURRENT_TEST_NAME.with(|slot| *slot.borrow_mut() = snapshot.name);
    CURRENT_DIAGNOSTICS.with(|slot| *slot.borrow_mut() = snapshot.diagnostics);
    CURRENT_SNAPSHOT_INDEX.with(|slot| slot.set(snapshot.snapshot_index));
    CURRENT_ASSERT_COUNT.with(|slot| slot.set(snapshot.assertion_count));
    CURRENT_PLAN.with(|slot| slot.set(snapshot.plan));
    CURRENT_TEST_OVERRIDE.with(|slot| slot.set(snapshot.directive));
}

fn take_directive_reason(
    mode: &mut TestMode,
    reason: &mut Option<String>,
    diagnostics: &mut Vec<String>,
) {
    let directive = CURRENT_TEST_OVERRIDE.with(|slot| slot.get());
    let prefix = match directive {
        TEST_OVERRIDE_SKIP => {
            *mode = TestMode::Skip;
            "# SKIP "
        }
        TEST_OVERRIDE_TODO => {
            *mode = TestMode::Todo;
            "# TODO "
        }
        _ => return,
    };
    if let Some(index) = diagnostics.iter().position(|line| line.starts_with(prefix)) {
        let line = diagnostics.remove(index);
        *reason = Some(line[prefix.len()..].to_string());
    }
}

extern "C" fn test_done(_closure: *const ClosureHeader, error: f64) -> f64 {
    DONE_CALLED.with(|called| called.set(true));
    if !is_undefined_value(error) && !JSValue::from_bits(error.to_bits()).is_null() {
        crate::exception::js_throw(error);
    }
    undefined_value()
}

fn call_test_callback(callback: f64, name: &str) -> Result<f64, f64> {
    if !is_callable_value(callback) {
        return Ok(undefined_value());
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let context = scope.root_nanbox_f64(test_context_value(name));
    let callback_ptr = raw_ptr_from_value(callback) as *const ClosureHeader;
    let arity = crate::closure::closure_length(callback_ptr).unwrap_or(0);
    catch_js(|| {
        if arity >= 2 {
            DONE_CALLED.with(|called| called.set(false));
            let done = scope.root_nanbox_f64(closure_value(test_done as *const u8, 1));
            crate::closure::js_closure_call2(
                callback_ptr,
                context.get_nanbox_f64(),
                done.get_nanbox_f64(),
            )
        } else {
            js_closure_call1(callback_ptr, context.get_nanbox_f64())
        }
    })
}

fn await_callback_result(result: f64) -> Result<(), f64> {
    if crate::promise::js_value_is_promise(result) == 0 {
        return Ok(());
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let promise = scope.root_nanbox_f64(result);
    loop {
        let ptr = raw_ptr_from_value(promise.get_nanbox_f64()) as *mut crate::promise::Promise;
        match crate::promise::js_promise_state(ptr) {
            1 => return Ok(()),
            2 => return Err(crate::promise::js_promise_reason(ptr)),
            _ => {
                if crate::promise::js_promise_run_microtasks() == 0 {
                    return Ok(());
                }
            }
        }
    }
}

fn call_and_await(callback: f64, name: &str) -> Result<(), f64> {
    let result = call_test_callback(callback, name)?;
    await_callback_result(result)
}

fn run_hook(callback: f64, context_name: &str) -> Result<(), f64> {
    if !is_callable_value(callback) {
        return Ok(());
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let context = scope.root_nanbox_f64(test_context_value(context_name));
    let callback_ptr = raw_ptr_from_value(callback) as *const ClosureHeader;
    let result = catch_js(|| js_closure_call1(callback_ptr, context.get_nanbox_f64()))?;
    await_callback_result(result)
}

fn ancestor_hooks(ancestors: &[usize], kind: HookKind) -> Vec<f64> {
    TEST_RUNNER.with(|runner| {
        let runner = runner.borrow();
        let ids: Box<dyn Iterator<Item = &usize>> = match kind {
            HookKind::AfterEach => Box::new(ancestors.iter().rev()),
            _ => Box::new(ancestors.iter()),
        };
        let mut out = Vec::new();
        for id in ids {
            let suite = &runner.suites[*id];
            out.extend(match kind {
                HookKind::Before => &suite.before,
                HookKind::After => &suite.after,
                HookKind::BeforeEach => &suite.before_each,
                HookKind::AfterEach => &suite.after_each,
            });
        }
        out
    })
}

fn finalize_pending(result: &mut TestResult) {
    if let Some(value) = result.pending.take() {
        if await_callback_result(value).is_err() {
            result.failed = true;
        }
    }
    for child in &mut result.children {
        finalize_pending(child);
        result.failed |= child.failed;
    }
}

fn execute_inline_subtest(def: TestDef) -> TestResult {
    let prior = enter_context(&def.name);
    ACTIVE_CHILDREN.with(|children| children.borrow_mut().push(Vec::new()));
    let callback_result = if def.mode == TestMode::Skip {
        Ok(undefined_value())
    } else {
        call_test_callback(def.callback, &def.name)
    };
    let children = ACTIVE_CHILDREN.with(|children| children.borrow_mut().pop().unwrap_or_default());
    let mut mode = def.mode;
    let mut reason = def.reason;
    let mut diagnostics = CURRENT_DIAGNOSTICS.with(|slot| std::mem::take(&mut *slot.borrow_mut()));
    take_directive_reason(&mut mode, &mut reason, &mut diagnostics);
    let count = CURRENT_ASSERT_COUNT.with(|slot| slot.get());
    let plan = CURRENT_PLAN.with(|slot| slot.get());
    let plan_failed = plan.is_some_and(|expected| expected != count);
    restore_context(prior);

    let (failed, pending) = match callback_result {
        Ok(value) if crate::promise::js_value_is_promise(value) != 0 => (plan_failed, Some(value)),
        Ok(_) => (plan_failed, None),
        Err(_) => (true, None),
    };
    TestResult {
        name: def.name,
        mode,
        reason,
        diagnostics,
        failed,
        pending,
        children,
        suite: false,
    }
}

fn execute_test(def: TestDef, ancestors: &[usize], blocked: bool) -> TestResult {
    if def.mode == TestMode::Skip {
        return TestResult {
            name: def.name,
            mode: def.mode,
            reason: def.reason,
            diagnostics: Vec::new(),
            failed: blocked,
            pending: None,
            children: Vec::new(),
            suite: false,
        };
    }

    let mut failed = blocked;
    let before_each = ancestor_hooks(ancestors, HookKind::BeforeEach);
    let after_each = ancestor_hooks(ancestors, HookKind::AfterEach);
    if !blocked {
        for hook in before_each {
            if run_hook(hook, &def.name).is_err() {
                failed = true;
                break;
            }
        }
    }

    let prior = enter_context(&def.name);
    ACTIVE_CHILDREN.with(|children| children.borrow_mut().push(Vec::new()));
    ACTIVE_ANCESTORS.with(|slot| *slot.borrow_mut() = ancestors.to_vec());
    if !failed {
        match call_test_callback(def.callback, &def.name) {
            Ok(value) => {
                if await_callback_result(value).is_err() {
                    failed = true;
                }
            }
            Err(_) => failed = true,
        }
    }
    let mut children =
        ACTIVE_CHILDREN.with(|children| children.borrow_mut().pop().unwrap_or_default());
    for child in &mut children {
        finalize_pending(child);
        failed |= child.failed;
    }
    let mut mode = def.mode;
    let mut reason = def.reason;
    let mut diagnostics = CURRENT_DIAGNOSTICS.with(|slot| std::mem::take(&mut *slot.borrow_mut()));
    take_directive_reason(&mut mode, &mut reason, &mut diagnostics);
    let assertion_count = CURRENT_ASSERT_COUNT.with(|slot| slot.get());
    let plan = CURRENT_PLAN.with(|slot| slot.get());
    failed |= plan.is_some_and(|expected| expected != assertion_count);
    restore_context(prior);
    restore_tracked_mocks();

    for hook in after_each {
        if run_hook(hook, &def.name).is_err() {
            failed = true;
        }
    }

    TestResult {
        name: def.name,
        mode,
        reason,
        diagnostics,
        failed,
        pending: None,
        children,
        suite: false,
    }
}

fn execute_node(node: RegisteredNode, ancestors: &[usize], blocked: bool) -> TestResult {
    match node {
        RegisteredNode::Test(id) => {
            let def = TEST_RUNNER.with(|runner| runner.borrow().tests[id].clone());
            execute_test(def, ancestors, blocked)
        }
        RegisteredNode::Suite(id) => execute_suite(id, ancestors, blocked),
    }
}

fn execute_suite(id: usize, ancestors: &[usize], blocked: bool) -> TestResult {
    let initial = TEST_RUNNER.with(|runner| runner.borrow().suites[id].clone());
    let mut failed = blocked;
    if initial.mode != TestMode::Skip && !blocked && is_callable_value(initial.callback) {
        let previous = TEST_RUNNER.with(|runner| {
            let mut runner = runner.borrow_mut();
            let previous = runner.current_suite;
            runner.current_suite = id;
            previous
        });
        if call_and_await(initial.callback, &initial.name).is_err() {
            failed = true;
        }
        TEST_RUNNER.with(|runner| runner.borrow_mut().current_suite = previous);
    }

    let suite = TEST_RUNNER.with(|runner| runner.borrow().suites[id].clone());
    if suite.mode != TestMode::Skip && !failed {
        for hook in suite.before.iter().copied() {
            if run_hook(hook, &suite.name).is_err() {
                failed = true;
                break;
            }
        }
    }

    let mut child_ancestors = ancestors.to_vec();
    child_ancestors.push(id);
    let children = suite
        .children
        .iter()
        .copied()
        .map(|node| execute_node(node, &child_ancestors, failed))
        .collect::<Vec<_>>();
    failed |= children.iter().any(|child| child.failed);

    if suite.mode != TestMode::Skip {
        for hook in suite.after.iter().copied() {
            if run_hook(hook, &suite.name).is_err() {
                failed = true;
            }
        }
    }
    TestResult {
        name: suite.name,
        mode: suite.mode,
        reason: suite.reason,
        diagnostics: Vec::new(),
        failed,
        pending: None,
        children,
        suite: true,
    }
}

fn result_is_selected(node: RegisteredNode, has_only: bool) -> bool {
    if !has_only {
        return true;
    }
    TEST_RUNNER.with(|runner| {
        let runner = runner.borrow();
        match node {
            RegisteredNode::Test(id) => runner.tests[id].mode == TestMode::Only,
            RegisteredNode::Suite(id) => runner.suites[id].mode == TestMode::Only,
        }
    })
}

fn has_root_only(nodes: &[RegisteredNode]) -> bool {
    nodes.iter().copied().any(|node| {
        TEST_RUNNER.with(|runner| {
            let runner = runner.borrow();
            match node {
                RegisteredNode::Test(id) => runner.tests[id].mode == TestMode::Only,
                RegisteredNode::Suite(id) => runner.suites[id].mode == TestMode::Only,
            }
        })
    })
}

fn suffix(result: &TestResult) -> String {
    match result.mode {
        TestMode::Skip => result
            .reason
            .as_ref()
            .map_or_else(|| " # SKIP".to_string(), |reason| format!(" # {reason}")),
        TestMode::Todo => result
            .reason
            .as_ref()
            .map_or_else(|| " # TODO".to_string(), |reason| format!(" # {reason}")),
        _ => String::new(),
    }
}

fn print_result(result: &TestResult, indent: usize) {
    let pad = " ".repeat(indent);
    if !result.children.is_empty() {
        println!("{pad}▶ {}", result.name);
        for child in &result.children {
            print_result(child, indent + 2);
        }
    }
    let marker = if result.failed {
        "✖"
    } else if result.mode == TestMode::Skip {
        "﹣"
    } else {
        "✔"
    };
    println!("{pad}{marker} {} (0ms){}", result.name, suffix(result));
    for diagnostic in &result.diagnostics {
        println!("{pad}ℹ {diagnostic}");
    }
}

fn collect_stats(result: &TestResult, stats: &mut Stats) {
    if result.suite {
        stats.suites += 1;
    } else {
        stats.tests += 1;
        if result.failed {
            stats.failed += 1;
        } else {
            match result.mode {
                TestMode::Skip => stats.skipped += 1,
                TestMode::Todo => stats.todo += 1,
                _ => stats.passed += 1,
            }
        }
    }
    for child in &result.children {
        collect_stats(child, stats);
    }
}

fn print_summary(stats: &Stats) {
    println!("ℹ tests {}", stats.tests);
    println!("ℹ suites {}", stats.suites);
    println!("ℹ pass {}", stats.passed);
    println!("ℹ fail {}", stats.failed);
    println!("ℹ cancelled 0");
    println!("ℹ skipped {}", stats.skipped);
    println!("ℹ todo {}", stats.todo);
    println!("ℹ duration_ms 0");
}

extern "C" fn test_runner_task(_closure: *const ClosureHeader) -> f64 {
    let root = TEST_RUNNER.with(|runner| {
        let mut runner = runner.borrow_mut();
        runner.running = true;
        runner.suites[0].clone()
    });
    let mut root_failed = false;
    for hook in root.before.iter().copied() {
        if run_hook(hook, "<root>").is_err() {
            root_failed = true;
            break;
        }
    }

    let has_only = has_root_only(&root.children);
    let results = root
        .children
        .iter()
        .copied()
        .filter(|node| result_is_selected(*node, has_only))
        .map(|node| execute_node(node, &[0], root_failed))
        .collect::<Vec<_>>();

    for hook in root.after.iter().copied() {
        if run_hook(hook, "<root>").is_err() {
            root_failed = true;
        }
    }

    let mut stats = Stats::default();
    for result in &results {
        print_result(result, 0);
        collect_stats(result, &mut stats);
    }
    print_summary(&stats);
    if root_failed || stats.failed != 0 {
        crate::process::js_process_exit_code_set(1.0);
    }
    TEST_RUNNER.with(|runner| runner.borrow_mut().running = false);
    undefined_value()
}

pub(crate) extern "C" fn thunk_test(
    _closure: *const ClosureHeader,
    name: f64,
    options: f64,
    callback: f64,
) -> f64 {
    register_test(TestMode::Normal, name, options, callback)
}

pub(crate) extern "C" fn thunk_test_skip(
    _closure: *const ClosureHeader,
    name: f64,
    options: f64,
    callback: f64,
) -> f64 {
    register_test(TestMode::Skip, name, options, callback)
}

pub(crate) extern "C" fn thunk_test_todo(
    _closure: *const ClosureHeader,
    name: f64,
    options: f64,
    callback: f64,
) -> f64 {
    register_test(TestMode::Todo, name, options, callback)
}

pub(crate) extern "C" fn thunk_test_only(
    _closure: *const ClosureHeader,
    name: f64,
    options: f64,
    callback: f64,
) -> f64 {
    register_test(TestMode::Only, name, options, callback)
}

pub(crate) extern "C" fn thunk_test_suite(
    _closure: *const ClosureHeader,
    name: f64,
    options: f64,
    callback: f64,
) -> f64 {
    register_suite(TestMode::Normal, name, options, callback)
}

pub(crate) extern "C" fn thunk_test_suite_skip(
    _closure: *const ClosureHeader,
    name: f64,
    options: f64,
    callback: f64,
) -> f64 {
    register_suite(TestMode::Skip, name, options, callback)
}

pub(crate) extern "C" fn thunk_test_suite_todo(
    _closure: *const ClosureHeader,
    name: f64,
    options: f64,
    callback: f64,
) -> f64 {
    register_suite(TestMode::Todo, name, options, callback)
}

pub(crate) extern "C" fn thunk_test_suite_only(
    _closure: *const ClosureHeader,
    name: f64,
    options: f64,
    callback: f64,
) -> f64 {
    register_suite(TestMode::Only, name, options, callback)
}

pub(crate) extern "C" fn thunk_test_before(_closure: *const ClosureHeader, callback: f64) -> f64 {
    register_hook(HookKind::Before, callback)
}

pub(crate) extern "C" fn thunk_test_after(_closure: *const ClosureHeader, callback: f64) -> f64 {
    register_hook(HookKind::After, callback)
}

pub(crate) extern "C" fn thunk_test_before_each(
    _closure: *const ClosureHeader,
    callback: f64,
) -> f64 {
    register_hook(HookKind::BeforeEach, callback)
}

pub(crate) extern "C" fn thunk_test_after_each(
    _closure: *const ClosureHeader,
    callback: f64,
) -> f64 {
    register_hook(HookKind::AfterEach, callback)
}

pub(crate) extern "C" fn thunk_test_run(_closure: *const ClosureHeader, _options: f64) -> f64 {
    let mut array = crate::array::js_array_alloc(0);
    for _ in 0..10 {
        array = crate::array::js_array_push_f64(array, undefined_value());
    }
    crate::node_stream::js_node_stream_readable_from(boxed_ptr(array))
}

#[no_mangle]
pub extern "C" fn js_node_test_register(name: f64, options: f64, callback: f64) -> f64 {
    thunk_test(std::ptr::null(), name, options, callback)
}

#[no_mangle]
pub extern "C" fn js_node_test_skip(name: f64, options: f64, callback: f64) -> f64 {
    thunk_test_skip(std::ptr::null(), name, options, callback)
}

#[no_mangle]
pub extern "C" fn js_node_test_todo(name: f64, options: f64, callback: f64) -> f64 {
    thunk_test_todo(std::ptr::null(), name, options, callback)
}

#[no_mangle]
pub extern "C" fn js_node_test_only(name: f64, options: f64, callback: f64) -> f64 {
    thunk_test_only(std::ptr::null(), name, options, callback)
}

#[no_mangle]
pub extern "C" fn js_node_test_suite(name: f64, options: f64, callback: f64) -> f64 {
    thunk_test_suite(std::ptr::null(), name, options, callback)
}

#[no_mangle]
pub extern "C" fn js_node_test_before(callback: f64) -> f64 {
    thunk_test_before(std::ptr::null(), callback)
}

#[no_mangle]
pub extern "C" fn js_node_test_after(callback: f64) -> f64 {
    thunk_test_after(std::ptr::null(), callback)
}

#[no_mangle]
pub extern "C" fn js_node_test_before_each(callback: f64) -> f64 {
    thunk_test_before_each(std::ptr::null(), callback)
}

#[no_mangle]
pub extern "C" fn js_node_test_after_each(callback: f64) -> f64 {
    thunk_test_after_each(std::ptr::null(), callback)
}

#[no_mangle]
pub extern "C" fn js_node_test_run(options: f64) -> f64 {
    thunk_test_run(std::ptr::null(), options)
}

pub(super) fn scan_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    TEST_RUNNER.with(|runner| {
        let mut runner = runner.borrow_mut();
        for test in &mut runner.tests {
            visitor.visit_nanbox_f64_slot(&mut test.callback);
        }
        for suite in &mut runner.suites {
            visitor.visit_nanbox_f64_slot(&mut suite.callback);
            for callback in &mut suite.before {
                visitor.visit_nanbox_f64_slot(callback);
            }
            for callback in &mut suite.after {
                visitor.visit_nanbox_f64_slot(callback);
            }
            for callback in &mut suite.before_each {
                visitor.visit_nanbox_f64_slot(callback);
            }
            for callback in &mut suite.after_each {
                visitor.visit_nanbox_f64_slot(callback);
            }
        }
    });
    ACTIVE_CHILDREN.with(|frames| {
        for frame in frames.borrow_mut().iter_mut() {
            for result in frame.iter_mut() {
                scan_result_roots(result, visitor);
            }
        }
    });
}

fn scan_result_roots(result: &mut TestResult, visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    if let Some(pending) = &mut result.pending {
        visitor.visit_nanbox_f64_slot(pending);
    }
    for child in &mut result.children {
        scan_result_roots(child, visitor);
    }
}
