//! The shallow copies of the url, object, reflect, template and plugin nodes (see [`super::shallow`]).

use super::*;

/// The copy of `e` with a hole (counted in `n`) at each sub-expression, for the
/// variants of this group; any other variant goes on to the next group.
#[allow(clippy::clone_on_copy)]
pub(super) fn shallow(e: &Expr, n: &mut usize) -> Expr {
    match e {
        Expr::UrlGetHref(a0) => Expr::UrlGetHref(Shallow::shallow(a0, n)),
        Expr::UrlGetPathname(a0) => Expr::UrlGetPathname(Shallow::shallow(a0, n)),
        Expr::UrlGetProtocol(a0) => Expr::UrlGetProtocol(Shallow::shallow(a0, n)),
        Expr::UrlGetHost(a0) => Expr::UrlGetHost(Shallow::shallow(a0, n)),
        Expr::UrlGetHostname(a0) => Expr::UrlGetHostname(Shallow::shallow(a0, n)),
        Expr::UrlGetPort(a0) => Expr::UrlGetPort(Shallow::shallow(a0, n)),
        Expr::UrlGetSearch(a0) => Expr::UrlGetSearch(Shallow::shallow(a0, n)),
        Expr::UrlGetHash(a0) => Expr::UrlGetHash(Shallow::shallow(a0, n)),
        Expr::UrlGetOrigin(a0) => Expr::UrlGetOrigin(Shallow::shallow(a0, n)),
        Expr::UrlGetSearchParams(a0) => Expr::UrlGetSearchParams(Shallow::shallow(a0, n)),
        Expr::UrlCanParse(a0) => Expr::UrlCanParse(Shallow::shallow(a0, n)),
        Expr::UrlCanParseWithBase { input, base } => Expr::UrlCanParseWithBase {
            input: Shallow::shallow(input, n),
            base: Shallow::shallow(base, n),
        },
        Expr::UrlParse(a0) => Expr::UrlParse(Shallow::shallow(a0, n)),
        Expr::UrlParseWithBase { input, base } => Expr::UrlParseWithBase {
            input: Shallow::shallow(input, n),
            base: Shallow::shallow(base, n),
        },
        Expr::UrlInstanceToString(a0) => Expr::UrlInstanceToString(Shallow::shallow(a0, n)),
        Expr::UrlInstanceToJSON(a0) => Expr::UrlInstanceToJSON(Shallow::shallow(a0, n)),
        Expr::UrlSetPathname { url, value } => Expr::UrlSetPathname {
            url: Shallow::shallow(url, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSetSearch { url, value } => Expr::UrlSetSearch {
            url: Shallow::shallow(url, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSetHash { url, value } => Expr::UrlSetHash {
            url: Shallow::shallow(url, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSetProtocol { url, value } => Expr::UrlSetProtocol {
            url: Shallow::shallow(url, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSetHostname { url, value } => Expr::UrlSetHostname {
            url: Shallow::shallow(url, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSetPort { url, value } => Expr::UrlSetPort {
            url: Shallow::shallow(url, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSetUsername { url, value } => Expr::UrlSetUsername {
            url: Shallow::shallow(url, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSetPassword { url, value } => Expr::UrlSetPassword {
            url: Shallow::shallow(url, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSetHref { url, value } => Expr::UrlSetHref {
            url: Shallow::shallow(url, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSearchParamsNew(a0) => Expr::UrlSearchParamsNew(Shallow::shallow(a0, n)),
        Expr::UrlSearchParamsMissingArgs {
            params,
            args,
            name_and_value,
        } => Expr::UrlSearchParamsMissingArgs {
            params: Shallow::shallow(params, n),
            args: Shallow::shallow(args, n),
            name_and_value: name_and_value.clone(),
        },
        Expr::UrlSearchParamsGet { params, name } => Expr::UrlSearchParamsGet {
            params: Shallow::shallow(params, n),
            name: Shallow::shallow(name, n),
        },
        Expr::UrlSearchParamsHas {
            params,
            name,
            value,
        } => Expr::UrlSearchParamsHas {
            params: Shallow::shallow(params, n),
            name: Shallow::shallow(name, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSearchParamsSet {
            params,
            name,
            value,
        } => Expr::UrlSearchParamsSet {
            params: Shallow::shallow(params, n),
            name: Shallow::shallow(name, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSearchParamsAppend {
            params,
            name,
            value,
        } => Expr::UrlSearchParamsAppend {
            params: Shallow::shallow(params, n),
            name: Shallow::shallow(name, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSearchParamsDelete {
            params,
            name,
            value,
        } => Expr::UrlSearchParamsDelete {
            params: Shallow::shallow(params, n),
            name: Shallow::shallow(name, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSearchParamsToString(a0) => Expr::UrlSearchParamsToString(Shallow::shallow(a0, n)),
        Expr::UrlSearchParamsGetAll { params, name } => Expr::UrlSearchParamsGetAll {
            params: Shallow::shallow(params, n),
            name: Shallow::shallow(name, n),
        },
        Expr::UrlSearchParamsEntries(a0) => Expr::UrlSearchParamsEntries(Shallow::shallow(a0, n)),
        Expr::UrlSearchParamsKeys(a0) => Expr::UrlSearchParamsKeys(Shallow::shallow(a0, n)),
        Expr::UrlSearchParamsValues(a0) => Expr::UrlSearchParamsValues(Shallow::shallow(a0, n)),
        Expr::UrlSearchParamsSort(a0) => Expr::UrlSearchParamsSort(Shallow::shallow(a0, n)),
        Expr::UrlSearchParamsForEach {
            params,
            callback,
            this_arg,
        } => Expr::UrlSearchParamsForEach {
            params: Shallow::shallow(params, n),
            callback: Shallow::shallow(callback, n),
            this_arg: Shallow::shallow(this_arg, n),
        },
        Expr::Delete(a0) => Expr::Delete(Shallow::shallow(a0, n)),
        Expr::Closure {
            func_id,
            params,
            return_type,
            body,
            captures,
            mutable_captures,
            captures_this,
            captures_new_target,
            enclosing_class,
            is_arrow,
            is_async,
            is_generator,
            is_strict,
        } => Expr::Closure {
            func_id: func_id.clone(),
            params: Shallow::shallow(params, n),
            return_type: return_type.clone(),
            body: body.clone(),
            captures: captures.clone(),
            mutable_captures: mutable_captures.clone(),
            captures_this: captures_this.clone(),
            captures_new_target: captures_new_target.clone(),
            enclosing_class: enclosing_class.clone(),
            is_arrow: is_arrow.clone(),
            is_async: is_async.clone(),
            is_generator: is_generator.clone(),
            is_strict: is_strict.clone(),
        },
        Expr::RegExp { pattern, flags } => Expr::RegExp {
            pattern: pattern.clone(),
            flags: flags.clone(),
        },
        Expr::RegExpDynamic {
            pattern,
            flags,
            is_call,
        } => Expr::RegExpDynamic {
            pattern: Shallow::shallow(pattern, n),
            flags: Shallow::shallow(flags, n),
            is_call: is_call.clone(),
        },
        Expr::StringMatch { string, regex } => Expr::StringMatch {
            string: Shallow::shallow(string, n),
            regex: Shallow::shallow(regex, n),
        },
        Expr::StringMatchAll { string, regex } => Expr::StringMatchAll {
            string: Shallow::shallow(string, n),
            regex: Shallow::shallow(regex, n),
        },
        Expr::StringReplace {
            string,
            pattern,
            replacement,
        } => Expr::StringReplace {
            string: Shallow::shallow(string, n),
            pattern: Shallow::shallow(pattern, n),
            replacement: Shallow::shallow(replacement, n),
        },
        Expr::ObjectFromEntries(a0) => Expr::ObjectFromEntries(Shallow::shallow(a0, n)),
        Expr::ObjectIs(a0, a1) => Expr::ObjectIs(Shallow::shallow(a0, n), Shallow::shallow(a1, n)),
        Expr::ObjectHasOwn(a0, a1) => {
            Expr::ObjectHasOwn(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::ObjectKeys(a0) => Expr::ObjectKeys(Shallow::shallow(a0, n)),
        Expr::ForInKeys(a0) => Expr::ForInKeys(Shallow::shallow(a0, n)),
        Expr::ObjectValues(a0) => Expr::ObjectValues(Shallow::shallow(a0, n)),
        Expr::ObjectEntries(a0) => Expr::ObjectEntries(Shallow::shallow(a0, n)),
        Expr::ObjectGroupBy { items, key_fn } => Expr::ObjectGroupBy {
            items: Shallow::shallow(items, n),
            key_fn: Shallow::shallow(key_fn, n),
        },
        Expr::MapGroupBy { items, key_fn } => Expr::MapGroupBy {
            items: Shallow::shallow(items, n),
            key_fn: Shallow::shallow(key_fn, n),
        },
        Expr::ObjectRest {
            object,
            exclude_keys,
        } => Expr::ObjectRest {
            object: Shallow::shallow(object, n),
            exclude_keys: exclude_keys.clone(),
        },
        Expr::ArrayIsArray(a0) => Expr::ArrayIsArray(Shallow::shallow(a0, n)),
        Expr::ArrayFrom(a0, a1) => Expr::ArrayFrom(Shallow::shallow(a0, n), a1.clone()),
        Expr::ArrayFromArrayLikeHoley(a0) => Expr::ArrayFromArrayLikeHoley(Shallow::shallow(a0, n)),
        Expr::IteratorFrom(a0) => Expr::IteratorFrom(Shallow::shallow(a0, n)),
        Expr::TaggedTemplateStrings {
            site_id,
            cooked,
            raw,
        } => Expr::TaggedTemplateStrings {
            site_id: site_id.clone(),
            cooked: Shallow::shallow(cooked, n),
            raw: raw.clone(),
        },
        Expr::TemplateRaw(a0) => Expr::TemplateRaw(Shallow::shallow(a0, n)),
        Expr::IteratorToArray(a0) => Expr::IteratorToArray(Shallow::shallow(a0, n)),
        Expr::GetIterator(a0) => Expr::GetIterator(Shallow::shallow(a0, n)),
        Expr::ArrayIterationPatched => Expr::ArrayIterationPatched,
        Expr::GetAsyncIterator(a0) => Expr::GetAsyncIterator(Shallow::shallow(a0, n)),
        Expr::ForOfToArray(a0) => Expr::ForOfToArray(Shallow::shallow(a0, n)),
        Expr::ForAwaitToArray(a0) => Expr::ForAwaitToArray(Shallow::shallow(a0, n)),
        Expr::ArrayFromMapped {
            iterable,
            map_fn,
            this_arg,
        } => Expr::ArrayFromMapped {
            iterable: Shallow::shallow(iterable, n),
            map_fn: Shallow::shallow(map_fn, n),
            this_arg: Shallow::shallow(this_arg, n),
        },
        Expr::ParseInt { string, radix } => Expr::ParseInt {
            string: Shallow::shallow(string, n),
            radix: Shallow::shallow(radix, n),
        },
        Expr::ParseFloat(a0) => Expr::ParseFloat(Shallow::shallow(a0, n)),
        Expr::NumberCoerce(a0) => Expr::NumberCoerce(Shallow::shallow(a0, n)),
        Expr::BigIntCoerce(a0) => Expr::BigIntCoerce(Shallow::shallow(a0, n)),
        Expr::StringCoerce(a0) => Expr::StringCoerce(Shallow::shallow(a0, n)),
        Expr::TemplateStringCoerce(a0) => Expr::TemplateStringCoerce(Shallow::shallow(a0, n)),
        Expr::ObjectCoerce(a0) => Expr::ObjectCoerce(Shallow::shallow(a0, n)),
        Expr::BooleanCoerce(a0) => Expr::BooleanCoerce(Shallow::shallow(a0, n)),
        Expr::IsNaN(a0) => Expr::IsNaN(Shallow::shallow(a0, n)),
        Expr::IsUndefinedOrBareNan(a0) => Expr::IsUndefinedOrBareNan(Shallow::shallow(a0, n)),
        Expr::IsFinite(a0) => Expr::IsFinite(Shallow::shallow(a0, n)),
        Expr::NumberIsNaN(a0) => Expr::NumberIsNaN(Shallow::shallow(a0, n)),
        Expr::NumberIsFinite(a0) => Expr::NumberIsFinite(Shallow::shallow(a0, n)),
        Expr::NumberIsInteger(a0) => Expr::NumberIsInteger(Shallow::shallow(a0, n)),
        Expr::NumberIsSafeInteger(a0) => Expr::NumberIsSafeInteger(Shallow::shallow(a0, n)),
        Expr::StaticPluginResolve(a0) => Expr::StaticPluginResolve(Shallow::shallow(a0, n)),
        Expr::JsLoadModule { path } => Expr::JsLoadModule { path: path.clone() },
        Expr::JsGetExport {
            module_handle,
            export_name,
        } => Expr::JsGetExport {
            module_handle: Shallow::shallow(module_handle, n),
            export_name: export_name.clone(),
        },
        Expr::JsCallFunction {
            module_handle,
            func_name,
            args,
        } => Expr::JsCallFunction {
            module_handle: Shallow::shallow(module_handle, n),
            func_name: func_name.clone(),
            args: Shallow::shallow(args, n),
        },
        Expr::JsCallMethod {
            object,
            method_name,
            args,
        } => Expr::JsCallMethod {
            object: Shallow::shallow(object, n),
            method_name: method_name.clone(),
            args: Shallow::shallow(args, n),
        },
        Expr::JsCallValue { callee, args } => Expr::JsCallValue {
            callee: Shallow::shallow(callee, n),
            args: Shallow::shallow(args, n),
        },
        Expr::JsGetProperty {
            object,
            property_name,
        } => Expr::JsGetProperty {
            object: Shallow::shallow(object, n),
            property_name: property_name.clone(),
        },
        Expr::JsSetProperty {
            object,
            property_name,
            value,
        } => Expr::JsSetProperty {
            object: Shallow::shallow(object, n),
            property_name: property_name.clone(),
            value: Shallow::shallow(value, n),
        },
        Expr::JsNew {
            module_handle,
            class_name,
            args,
        } => Expr::JsNew {
            module_handle: Shallow::shallow(module_handle, n),
            class_name: class_name.clone(),
            args: Shallow::shallow(args, n),
        },
        Expr::JsNewFromHandle { constructor, args } => Expr::JsNewFromHandle {
            constructor: Shallow::shallow(constructor, n),
            args: Shallow::shallow(args, n),
        },
        Expr::JsCreateCallback {
            closure,
            param_count,
        } => Expr::JsCreateCallback {
            closure: Shallow::shallow(closure, n),
            param_count: param_count.clone(),
        },
        Expr::ImportMetaUrl(a0) => Expr::ImportMetaUrl(a0.clone()),
        Expr::ProxyNew { target, handler } => Expr::ProxyNew {
            target: Shallow::shallow(target, n),
            handler: Shallow::shallow(handler, n),
        },
        Expr::ProxyGet { proxy, key } => Expr::ProxyGet {
            proxy: Shallow::shallow(proxy, n),
            key: Shallow::shallow(key, n),
        },
        Expr::ProxySet { proxy, key, value } => Expr::ProxySet {
            proxy: Shallow::shallow(proxy, n),
            key: Shallow::shallow(key, n),
            value: Shallow::shallow(value, n),
        },
        Expr::ProxyHas { proxy, key } => Expr::ProxyHas {
            proxy: Shallow::shallow(proxy, n),
            key: Shallow::shallow(key, n),
        },
        Expr::ProxyDelete { proxy, key } => Expr::ProxyDelete {
            proxy: Shallow::shallow(proxy, n),
            key: Shallow::shallow(key, n),
        },
        Expr::ProxyApply { proxy, args } => Expr::ProxyApply {
            proxy: Shallow::shallow(proxy, n),
            args: Shallow::shallow(args, n),
        },
        Expr::ProxyConstruct { proxy, args } => Expr::ProxyConstruct {
            proxy: Shallow::shallow(proxy, n),
            args: Shallow::shallow(args, n),
        },
        Expr::ProxyRevocable { target, handler } => Expr::ProxyRevocable {
            target: Shallow::shallow(target, n),
            handler: Shallow::shallow(handler, n),
        },
        Expr::ProxyRevoke(a0) => Expr::ProxyRevoke(Shallow::shallow(a0, n)),
        Expr::ReflectGet {
            target,
            key,
            receiver,
        } => Expr::ReflectGet {
            target: Shallow::shallow(target, n),
            key: Shallow::shallow(key, n),
            receiver: Shallow::shallow(receiver, n),
        },
        Expr::ReflectSet {
            target,
            key,
            value,
            receiver,
        } => Expr::ReflectSet {
            target: Shallow::shallow(target, n),
            key: Shallow::shallow(key, n),
            value: Shallow::shallow(value, n),
            receiver: Shallow::shallow(receiver, n),
        },
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            strict,
            element_offset,
            byte_offset,
            byte_end,
        } => Expr::PutValueSet {
            target: Shallow::shallow(target, n),
            key: Shallow::shallow(key, n),
            value: Shallow::shallow(value, n),
            receiver: Shallow::shallow(receiver, n),
            strict: strict.clone(),
            element_offset: element_offset.clone(),
            byte_offset: byte_offset.clone(),
            byte_end: byte_end.clone(),
        },
        Expr::ReflectHas { target, key } => Expr::ReflectHas {
            target: Shallow::shallow(target, n),
            key: Shallow::shallow(key, n),
        },
        Expr::ReflectDelete { target, key } => Expr::ReflectDelete {
            target: Shallow::shallow(target, n),
            key: Shallow::shallow(key, n),
        },
        Expr::ReflectOwnKeys(a0) => Expr::ReflectOwnKeys(Shallow::shallow(a0, n)),
        Expr::ReflectApply {
            func,
            this_arg,
            args,
        } => Expr::ReflectApply {
            func: Shallow::shallow(func, n),
            this_arg: Shallow::shallow(this_arg, n),
            args: Shallow::shallow(args, n),
        },
        Expr::ReflectConstruct {
            target,
            args,
            new_target,
        } => Expr::ReflectConstruct {
            target: Shallow::shallow(target, n),
            args: Shallow::shallow(args, n),
            new_target: Shallow::shallow(new_target, n),
        },
        Expr::ReflectDefineProperty {
            target,
            key,
            descriptor,
        } => Expr::ReflectDefineProperty {
            target: Shallow::shallow(target, n),
            key: Shallow::shallow(key, n),
            descriptor: Shallow::shallow(descriptor, n),
        },
        Expr::ReflectGetOwnPropertyDescriptor { target, key } => {
            Expr::ReflectGetOwnPropertyDescriptor {
                target: Shallow::shallow(target, n),
                key: Shallow::shallow(key, n),
            }
        }
        Expr::ReflectGetPrototypeOf(a0) => Expr::ReflectGetPrototypeOf(Shallow::shallow(a0, n)),
        Expr::ReflectSetPrototypeOf { target, proto } => Expr::ReflectSetPrototypeOf {
            target: Shallow::shallow(target, n),
            proto: Shallow::shallow(proto, n),
        },
        Expr::ReflectIsExtensible(a0) => Expr::ReflectIsExtensible(Shallow::shallow(a0, n)),
        Expr::ReflectPreventExtensions(a0) => {
            Expr::ReflectPreventExtensions(Shallow::shallow(a0, n))
        }
        Expr::ReflectDefineMetadata {
            key,
            value,
            target,
            property_key,
        } => Expr::ReflectDefineMetadata {
            key: Shallow::shallow(key, n),
            value: Shallow::shallow(value, n),
            target: Shallow::shallow(target, n),
            property_key: Shallow::shallow(property_key, n),
        },
        Expr::ReflectGetMetadata {
            key,
            target,
            property_key,
        } => Expr::ReflectGetMetadata {
            key: Shallow::shallow(key, n),
            target: Shallow::shallow(target, n),
            property_key: Shallow::shallow(property_key, n),
        },
        Expr::ReflectGetOwnMetadata {
            key,
            target,
            property_key,
        } => Expr::ReflectGetOwnMetadata {
            key: Shallow::shallow(key, n),
            target: Shallow::shallow(target, n),
            property_key: Shallow::shallow(property_key, n),
        },
        Expr::ReflectHasMetadata {
            key,
            target,
            property_key,
        } => Expr::ReflectHasMetadata {
            key: Shallow::shallow(key, n),
            target: Shallow::shallow(target, n),
            property_key: Shallow::shallow(property_key, n),
        },
        Expr::ReflectHasOwnMetadata {
            key,
            target,
            property_key,
        } => Expr::ReflectHasOwnMetadata {
            key: Shallow::shallow(key, n),
            target: Shallow::shallow(target, n),
            property_key: Shallow::shallow(property_key, n),
        },
        Expr::ReflectGetMetadataKeys {
            target,
            property_key,
        } => Expr::ReflectGetMetadataKeys {
            target: Shallow::shallow(target, n),
            property_key: Shallow::shallow(property_key, n),
        },
        Expr::ReflectGetOwnMetadataKeys {
            target,
            property_key,
        } => Expr::ReflectGetOwnMetadataKeys {
            target: Shallow::shallow(target, n),
            property_key: Shallow::shallow(property_key, n),
        },
        Expr::ReflectDeleteMetadata {
            key,
            target,
            property_key,
        } => Expr::ReflectDeleteMetadata {
            key: Shallow::shallow(key, n),
            target: Shallow::shallow(target, n),
            property_key: Shallow::shallow(property_key, n),
        },
        Expr::DynamicImport {
            paths,
            arg,
            options,
            byte_offset,
            deferred_error,
            synchronous,
        } => Expr::DynamicImport {
            paths: paths.clone(),
            arg: Shallow::shallow(arg, n),
            options: Shallow::shallow(options, n),
            byte_offset: byte_offset.clone(),
            deferred_error: deferred_error.clone(),
            synchronous: synchronous.clone(),
        },
        Expr::WorkerNew {
            paths,
            partial,
            filename,
            options,
            is_eval,
        } => Expr::WorkerNew {
            paths: paths.clone(),
            partial: partial.clone(),
            filename: Shallow::shallow(filename, n),
            options: Shallow::shallow(options, n),
            is_eval: is_eval.clone(),
        },
        _ => unreachable!("internal compiler error: Expr::clone: not a web variant"),
    }
}
