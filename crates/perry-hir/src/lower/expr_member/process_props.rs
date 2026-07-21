//! process.* / WebSocket-readyState property helpers for member lowering.
//!
//! Split out of `expr_member.rs` (pure code move).

use swc_ecma_ast as ast;

use crate::ir::Expr;

use super::LoweringContext;

/// #3946: lower a value-read of a `node:process` core property imported by
/// name (`import { pid, arch } from "node:process"`) or read off a namespace
/// local. Mirrors the dedicated `process.<prop>` variants used by the global
/// member-access path so named/namespace forms agree with `process.<prop>`
/// instead of resolving to `undefined`. Methods (`cwd`, `exit`, …) return
/// `None` so the caller keeps lowering them to a callable native-module ref.
pub(crate) fn lower_process_named_property(prop: &str) -> Option<Expr> {
    Some(match prop {
        "argv" => Expr::ProcessArgv,
        "platform" => Expr::OsPlatform,
        "arch" => Expr::OsArch,
        "pid" => Expr::ProcessPid,
        "ppid" => Expr::ProcessPpid,
        "version" => Expr::ProcessVersion,
        "versions" => Expr::ProcessVersions,
        "env" => Expr::ProcessEnv,
        "stdin" => Expr::ProcessStdin,
        "stdout" => Expr::ProcessStdout,
        "stderr" => Expr::ProcessStderr,
        _ => return process_metadata_native_property(prop),
    })
}

pub(crate) fn process_native_property(prop: &str) -> Expr {
    Expr::PropertyGet {
        byte_offset: 0,
        object: Box::new(Expr::NativeModuleRef("process".to_string())),
        property: prop.to_string(),
    }
}

pub(crate) fn process_metadata_native_property(prop: &str) -> Option<Expr> {
    Some(match prop {
        "allowedNodeEnvironmentFlags"
        | "argv0"
        | "channel"
        | "config"
        | "connected"
        | "debugPort"
        | "disconnect"
        | "execArgv"
        | "execPath"
        | "features"
        | "finalization"
        | "moduleLoadList"
        | "permission"
        | "release"
        | "report"
        | "send"
        | "sourceMapsEnabled"
        | "title" => process_native_property(prop),
        _ => return None,
    })
}

pub(crate) fn ws_ready_state_value(prop: &str) -> Option<f64> {
    Some(match prop {
        "CONNECTING" => 0.0,
        "OPEN" => 1.0,
        "CLOSING" => 2.0,
        "CLOSED" => 3.0,
        _ => return None,
    })
}

pub(crate) fn is_ws_ready_state_receiver(
    ctx: &LoweringContext,
    obj_ast: &ast::Expr,
    object_expr: &Expr,
) -> bool {
    fn native_ws_class_property(expr: &Expr) -> bool {
        match expr {
            Expr::NativeModuleRef(module) if module == "ws" => true,
            Expr::PropertyGet {
                object, property, ..
            } if matches!(property.as_str(), "WebSocket" | "default")
                && matches!(object.as_ref(), Expr::NativeModuleRef(module) if module == "ws") =>
            {
                true
            }
            Expr::PropertyGet {
                object, property, ..
            } if property == "WebSocket" && matches!(object.as_ref(), Expr::GlobalGet(0)) => true,
            _ => false,
        }
    }

    if native_ws_class_property(object_expr) {
        return true;
    }

    let ast::Expr::Ident(obj_ident) = obj_ast else {
        return false;
    };
    matches!(
        ctx.lookup_native_module(obj_ident.sym.as_ref()),
        Some(("ws", None | Some("default") | Some("WebSocket")))
    )
}
