//! Bounded interpretation of the small, pure helpers used by bundled Workers.
//!
//! This only discovers an import edge. Codegen still evaluates the original
//! filename expression, including its helper calls, when constructing a Worker.
use super::*;

const DEPTH_LIMIT: usize = 64;
const WORK_LIMIT: usize = 4096;
const STRING_LIMIT: usize = 65_536;
type Paths = Result<PathValues, String>;

// A URL is a carrier for a module edge, not its relative input string. Keep
// that distinction when substituting arguments: stringifying a URL would use
// its absolute href and must not silently concatenate the lexical input.
#[derive(Clone)]
struct PathValues {
    paths: Vec<String>,
    is_url: bool,
}

impl PathValues {
    fn strings(paths: Vec<String>) -> Self {
        Self {
            paths,
            is_url: false,
        }
    }
}

/// Extend the existing path grammar for Workers with local helper calls. Keep
/// dynamic import and eval-source resolution on their existing code paths.
pub fn resolve_worker_path<V: Borrow<Expr>>(
    arg: &Expr,
    module: &Module,
    consts: &HashMap<u32, V>,
    param_literals: &HashMap<u32, Vec<String>>,
    local_literals: &HashMap<u32, Vec<String>>,
) -> Resolution {
    let original = resolve_import_path_with_context(
        arg,
        consts,
        param_literals,
        local_literals,
        &mut HashSet::new(),
    );
    if matches!(original, Resolution::Set(_)) {
        return original;
    }
    let mut resolver = WorkerPaths {
        module,
        consts,
        param_literals,
        local_literals,
        arguments: HashMap::new(),
        locals: HashSet::new(),
        calls: HashSet::new(),
        work: WORK_LIMIT,
    };
    match resolver.resolve(arg, 0) {
        Ok(values) => Resolution::Set(values.paths),
        Err(reason) => Resolution::Unresolved(format!("Worker path helper: {reason}")),
    }
}

struct WorkerPaths<'a, V> {
    module: &'a Module,
    consts: &'a HashMap<u32, V>,
    param_literals: &'a HashMap<u32, Vec<String>>,
    local_literals: &'a HashMap<u32, Vec<String>>,
    arguments: HashMap<u32, PathValues>,
    locals: HashSet<u32>,
    calls: HashSet<u32>,
    work: usize,
}

impl<V: Borrow<Expr>> WorkerPaths<'_, V> {
    fn tick(&mut self, depth: usize) -> Result<(), String> {
        if depth >= DEPTH_LIMIT {
            return Err(format!("resolution exceeds depth limit {DEPTH_LIMIT}"));
        }
        spend(&mut self.work)
    }

    fn strings(&mut self, expr: &Expr, depth: usize) -> Result<Vec<String>, String> {
        let values = self.resolve(expr, depth)?;
        if values.is_url {
            return Err("URL string coercion is not a static path operation".into());
        }
        Ok(values.paths)
    }

    fn resolve(&mut self, expr: &Expr, depth: usize) -> Paths {
        self.tick(depth)?;
        match expr {
            Expr::String(value) => bounded(vec![value.clone()], &mut self.work),
            Expr::StringCoerce(value) => self.strings(value, depth + 1).map(PathValues::strings),
            Expr::LocalGet(id) => {
                if let Some(values) = self.arguments.get(id) {
                    return Ok(values.clone());
                }
                if !self.locals.insert(*id) {
                    return Err("circular binding reference".into());
                }
                let result = if let Some(init) = self.consts.get(id) {
                    self.resolve(init.borrow(), depth + 1)
                } else if self.calls.is_empty() {
                    self.param_literals
                        .get(id)
                        .or_else(|| self.local_literals.get(id))
                        .cloned()
                        .ok_or_else(|| "binding is mutable or has no static string value".into())
                        .and_then(|paths| bounded(paths, &mut self.work))
                } else {
                    Err("helper reads a mutable or non-static binding".into())
                };
                self.locals.remove(id);
                result
            }
            Expr::UrlNew { url, base } => {
                let paths = self.strings(url, depth + 1)?;
                match base {
                    Some(base) if matches!(base.as_ref(), Expr::ImportMetaUrl(_)) => {
                        Ok(PathValues {
                            paths,
                            is_url: true,
                        })
                    }
                    Some(base) => {
                        let bases = self.strings(base, depth + 1)?;
                        if bases.iter().all(|base| base.starts_with("file:")) {
                            Ok(PathValues {
                                paths,
                                is_url: true,
                            })
                        } else {
                            Err("URL base must be import.meta.url or a static file URL".into())
                        }
                    }
                    None if paths.iter().all(|path| path.starts_with("file:")) => Ok(PathValues {
                        paths,
                        is_url: true,
                    }),
                    None => Err("one-argument URL must resolve to a static file URL".into()),
                }
            }
            Expr::Binary {
                op: BinaryOp::Add,
                left,
                right,
            } => {
                let left = self.strings(left, depth + 1)?;
                let right = self.strings(right, depth + 1)?;
                product(&left, &right, |a, b| format!("{a}{b}"), &mut self.work)
            }
            Expr::PathJoin(left, right) | Expr::PathResolveJoin(left, right) => {
                let left = self.strings(left, depth + 1)?;
                let right = self.strings(right, depth + 1)?;
                product(&left, &right, static_path_join, &mut self.work)
            }
            Expr::StringReplace {
                string,
                pattern,
                replacement,
            } => self.replace(string, pattern, replacement, depth + 1),
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                self.pure_selector(condition, depth + 1)?;
                let mut then_values = self.resolve(then_expr, depth + 1)?;
                let else_values = self.resolve(else_expr, depth + 1)?;
                if then_values.is_url != else_values.is_url {
                    return Err("conditional mixes URL and string values".into());
                }
                then_values.paths.extend(else_values.paths);
                let mut values = bounded(then_values.paths, &mut self.work)?;
                values.is_url = then_values.is_url;
                Ok(values)
            }
            Expr::Call { callee, args, .. } => {
                if let Some(string) = static_string_replace_target(callee, args) {
                    return self.replace(string, &args[0], &args[1], depth + 1);
                }
                if is_static_path_join_call(callee) {
                    let mut paths = vec![String::new()];
                    for arg in args {
                        let next = self.strings(arg, depth + 1)?;
                        paths = product(&paths, &next, static_path_join, &mut self.work)?.paths;
                    }
                    return Ok(PathValues::strings(if args.is_empty() {
                        vec![".".into()]
                    } else {
                        paths
                    }));
                }
                self.call(callee, args, depth + 1)
            }
            Expr::PropertyGet { object, .. } | Expr::IndexGet { object, .. } => {
                if let Expr::IndexGet { index, .. } = expr {
                    self.pure_selector(index, depth + 1)?;
                }
                self.registry(object, depth + 1)
            }
            _ => Err(
                "unsupported expression (effects, mutation and opaque calls are not evaluated)"
                    .into(),
            ),
        }
    }

    fn replace(
        &mut self,
        string: &Expr,
        pattern: &Expr,
        replacement: &Expr,
        depth: usize,
    ) -> Paths {
        let strings = self.strings(string, depth)?;
        let patterns = self.strings(pattern, depth)?;
        let replacements = self.strings(replacement, depth)?;
        let mut paths = Vec::new();
        for string in &strings {
            for pattern in &patterns {
                for replacement in &replacements {
                    push_path(
                        &mut paths,
                        string.replacen(pattern, replacement, 1),
                        &mut self.work,
                    )?;
                }
            }
        }
        Ok(PathValues::strings(paths))
    }

    fn registry(&mut self, object: &Expr, depth: usize) -> Paths {
        self.tick(depth)?;
        let values: Vec<&Expr> = match object {
            Expr::LocalGet(id) => {
                if !self.locals.insert(*id) {
                    return Err("circular registry reference".into());
                }
                let result = match self.consts.get(id) {
                    Some(init) => self.registry(init.borrow(), depth + 1),
                    None => Err("registry binding is mutable or opaque".into()),
                };
                self.locals.remove(id);
                return result;
            }
            Expr::Object(entries) => entries.iter().map(|(_, value)| value).collect(),
            Expr::New {
                class_name, args, ..
            } if class_name.starts_with("__AnonShape_") => args.iter().collect(),
            _ => return Err("member access is not a static path registry".into()),
        };
        let mut paths = Vec::new();
        for value in values {
            for path in self.strings(value, depth + 1)? {
                if !is_relative_specifier(&path) {
                    return Err("registry values must be relative module paths".into());
                }
                push_path(&mut paths, path, &mut self.work)?;
            }
        }
        Ok(PathValues::strings(paths))
    }

    // Do not discard effects hidden in ternary conditions or registry indices.
    fn pure_selector(&mut self, expr: &Expr, depth: usize) -> Result<(), String> {
        self.tick(depth)?;
        match expr {
            Expr::Bool(_) | Expr::String(_) | Expr::Integer(_) | Expr::Number(_) => Ok(()),
            Expr::LocalGet(id) if self.arguments.contains_key(id) => Ok(()),
            Expr::LocalGet(id) => match self.consts.get(id) {
                Some(init) => self.pure_selector(init.borrow(), depth + 1),
                None => Err("selector reads a mutable or non-static binding".into()),
            },
            _ => Err("selector may have side effects or is not static".into()),
        }
    }

    fn call(&mut self, callee: &Expr, args: &[Expr], depth: usize) -> Paths {
        self.tick(depth)?;
        let mut target = callee;
        let mut aliases = HashSet::new();
        while let Expr::LocalGet(id) = target {
            self.tick(depth + aliases.len())?;
            if !aliases.insert(*id) {
                return Err("circular callable binding".into());
            }
            target = self
                .consts
                .get(id)
                .ok_or("call target is mutable or is not a module-local helper")?
                .borrow();
        }
        let (id, params, body, asynchronous) = match target {
            Expr::FuncRef(id) => {
                let function = self
                    .module
                    .functions
                    .iter()
                    .find(|function| function.id == *id)
                    .ok_or("call target is not a module-local helper")?;
                (
                    *id,
                    &function.params,
                    &function.body,
                    function.is_async || function.is_generator || function.was_plain_async,
                )
            }
            Expr::Closure {
                func_id,
                params,
                body,
                is_async,
                is_generator,
                ..
            } => (*func_id, params, body, *is_async || *is_generator),
            _ => return Err("opaque call target is not a module-local helper".into()),
        };
        if asynchronous {
            return Err("async and generator helpers are not static path helpers".into());
        }
        if params.len() != args.len()
            || params.iter().any(|p| {
                p.default.is_some()
                    || p.is_rest
                    || p.arguments_object.is_some()
                    || !p.decorators.is_empty()
            })
        {
            return Err(
                "helper requires an exact list of simple static string/URL arguments".into(),
            );
        }
        let [Stmt::Return(Some(value))] = body.as_slice() else {
            return Err("helper body must contain only a single return (no effects, mutation or multiple returns)".into());
        };
        // Resolve arguments before entering the callee so sibling/nested calls
        // such as identity(identity(path)) are not mistaken for recursion.
        let mut bindings = Vec::new();
        for (param, arg) in params.iter().zip(args) {
            bindings.push((param.id, self.resolve(arg, depth + 1)?));
        }
        if !self.calls.insert(id) {
            return Err("recursive helper call".into());
        }
        let saved: Vec<_> = bindings
            .into_iter()
            .map(|(id, paths)| (id, self.arguments.insert(id, paths)))
            .collect();
        let result = self.resolve(value, depth + 1);
        for (id, previous) in saved {
            if let Some(previous) = previous {
                self.arguments.insert(id, previous);
            } else {
                self.arguments.remove(&id);
            }
        }
        self.calls.remove(&id);
        result
    }
}

fn spend(work: &mut usize) -> Result<(), String> {
    *work = work
        .checked_sub(1)
        .ok_or_else(|| format!("resolution exceeds work limit {WORK_LIMIT}"))?;
    Ok(())
}

fn push_path(paths: &mut Vec<String>, path: String, work: &mut usize) -> Result<(), String> {
    spend(work)?;
    if path.len() > STRING_LIMIT {
        return Err(format!(
            "resolved path exceeds string length limit {STRING_LIMIT}"
        ));
    }
    if !paths.contains(&path) {
        if paths.len() == DYNAMIC_IMPORT_PATH_CAP {
            return Err(format!(
                "candidate count exceeds limit {DYNAMIC_IMPORT_PATH_CAP}"
            ));
        }
        paths.push(path);
    }
    Ok(())
}

fn bounded(paths: Vec<String>, work: &mut usize) -> Paths {
    let mut out = Vec::new();
    for path in paths {
        push_path(&mut out, path, work)?;
    }
    Ok(PathValues::strings(out))
}

fn product(
    left: &[String],
    right: &[String],
    combine: impl Fn(&str, &str) -> String,
    work: &mut usize,
) -> Paths {
    let mut out = Vec::new();
    for a in left {
        for b in right {
            push_path(&mut out, combine(a, b), work)?;
        }
    }
    Ok(PathValues::strings(out))
}

#[cfg(test)]
mod tests;
