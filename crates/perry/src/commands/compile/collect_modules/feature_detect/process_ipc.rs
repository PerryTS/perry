//! IPC reachability of process listeners, without materializing analysis state.
use perry_hir::walker::walk_expr_children;
use perry_hir::{Expr, Module};

fn listener_method(method: &str) -> bool {
    matches!(
        method,
        "on" | "once" | "addListener" | "prependListener" | "prependOnceListener"
    )
}

fn local_event(event: &Expr) -> bool {
    matches!(event, Expr::String(name) if matches!(name.as_str(),
        "exit" | "beforeExit" | "SIGINT" | "SIGTERM" | "SIGHUP" | "SIGQUIT"
        | "SIGUSR1" | "SIGUSR2" | "SIGWINCH" | "SIGBREAK" | "SIGABRT"
        | "SIGALRM" | "SIGCHLD" | "SIGCONT" | "SIGIO" | "SIGPIPE"
        | "SIGPROF" | "SIGTSTP" | "SIGTTIN" | "SIGTTOU" | "SIGURG"
        | "SIGVTALRM" | "SIGXCPU" | "SIGXFSZ"))
}

fn lexical_arrow_value(mut expr: &Expr) -> bool {
    // A local assignment evaluates to its RHS without invoking a setter. Prove
    // the value in place; do not follow bindings or accept property assignments.
    // The normal expression walk still visits every assignment and closure body.
    while let Expr::LocalSet(_, value) = expr {
        expr = value;
    }
    matches!(expr, Expr::Closure { is_arrow: true, .. })
}

fn isolated_listener(args: &[Expr], discarded: bool) -> bool {
    // Ordinary callbacks receive process as `this`, even with a static signal.
    // Unknown callbacks and observable return values must keep the namespace.
    discarded
        && matches!(args, [event, handler] if local_event(event) && lexical_arrow_value(handler))
}

fn expression(expr: &Expr, discarded: bool) -> bool {
    match expr {
        Expr::NativeModuleRef(module)
            if matches!(
                module.as_str(),
                "process" | "process.default" | "process.namespace"
            ) =>
        {
            return true
        }
        Expr::PropertyGet { property, .. } if property == "process" => return true,
        Expr::IndexGet { index, .. } if matches!(index.as_ref(), Expr::String(key) if key == "process") => {
            return true
        }
        Expr::NativeMethodCall {
            module,
            method,
            args,
            ..
        } if module == "process" && listener_method(method) => {
            if !isolated_listener(args, discarded) {
                return true;
            }
        }
        Expr::ProcessOn { event, handler } | Expr::ProcessOnce { event, handler } => {
            if !(discarded && local_event(event) && lexical_arrow_value(handler)) {
                return true;
            }
        }
        Expr::Call { callee, args, .. } => {
            if let Expr::PropertyGet {
                object, property, ..
            } = callee.as_ref()
            {
                if matches!(object.as_ref(), Expr::GlobalGet(0)) && listener_method(property) {
                    if !isolated_listener(args, discarded) {
                        return true;
                    }
                    // Skip only the proven direct callee. Still inspect callback
                    // defaults and bodies for IPC, process escapes and nesting.
                    return args.iter().any(|arg| expression(arg, false));
                }
            }
        }
        Expr::PropertyGet {
            object, property, ..
        } if matches!(object.as_ref(), Expr::GlobalGet(0)) && listener_method(property) => {
            return true
        }
        Expr::Closure { body, .. } if super::usage::statements(body, &mut expression) => {
            return true
        }
        _ => {}
    }
    let mut found = false;
    walk_expr_children(expr, &mut |child| found |= expression(child, false));
    found
}

pub(super) fn needed(module: &Module) -> bool {
    super::global_object::may_escape(module) || super::usage::module(module, &mut expression)
}
