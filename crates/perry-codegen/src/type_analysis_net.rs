use perry_hir::types::Type as HirType;
use perry_hir::Expr;

fn native_result_class(module: &str, method: &str) -> Option<&'static str> {
    if module.strip_prefix("node:").unwrap_or(module) != "net" {
        return None;
    }
    match method {
        "Socket" | "Stream" | "connect" | "createConnection" => Some("Socket"),
        "Server" | "createServer" => Some("Server"),
        "BlockList" => Some("BlockList"),
        "SocketAddress" | "parse" => Some("SocketAddress"),
        _ => None,
    }
}

pub(crate) fn net_result_class(expr: &Expr) -> Option<&'static str> {
    match expr {
        Expr::NetCreateServer { .. } => Some("Server"),
        Expr::NativeMethodCall {
            module,
            method,
            object: None,
            ..
        } => native_result_class(module, method),
        _ => None,
    }
}

pub(crate) fn net_result_type(expr: &Expr) -> Option<HirType> {
    net_result_class(expr).map(|name| HirType::Named(name.to_string()))
}
