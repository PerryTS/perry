use super::*;

/// Substitute types in an expression
pub(crate) fn substitute_expr(expr: &Expr, substitutions: &HashMap<String, Type>) -> Expr {
    match expr {
        // Literals don't need substitution
        Expr::Undefined
        | Expr::Null
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::BigInt(_)
        | Expr::String(_) => expr.clone(),

        // Variables
        Expr::LocalGet(id) => Expr::LocalGet(*id),
        Expr::LocalSet(id, val) => {
            Expr::LocalSet(*id, Box::new(substitute_expr(val, substitutions)))
        }
        Expr::GlobalGet(id) => Expr::GlobalGet(*id),
        Expr::GlobalSet(id, val) => {
            Expr::GlobalSet(*id, Box::new(substitute_expr(val, substitutions)))
        }

        // Update
        Expr::Update { id, op, prefix } => Expr::Update {
            id: *id,
            op: *op,
            prefix: *prefix,
        },

        // Operations
        Expr::Binary { op, left, right } => Expr::Binary {
            op: *op,
            left: Box::new(substitute_expr(left, substitutions)),
            right: Box::new(substitute_expr(right, substitutions)),
        },
        Expr::Unary { op, operand } => Expr::Unary {
            op: *op,
            operand: Box::new(substitute_expr(operand, substitutions)),
        },
        Expr::Compare { op, left, right } => Expr::Compare {
            op: *op,
            left: Box::new(substitute_expr(left, substitutions)),
            right: Box::new(substitute_expr(right, substitutions)),
        },
        Expr::Logical { op, left, right } => Expr::Logical {
            op: *op,
            left: Box::new(substitute_expr(left, substitutions)),
            right: Box::new(substitute_expr(right, substitutions)),
        },

        // Function call
        Expr::Call {
            callee,
            args,
            type_args,
            byte_offset,
        } => Expr::Call {
            callee: Box::new(substitute_expr(callee, substitutions)),
            args: args
                .iter()
                .map(|a| substitute_expr(a, substitutions))
                .collect(),
            type_args: type_args
                .iter()
                .map(|t| substitute_type(t, substitutions))
                .collect(),
            byte_offset: *byte_offset,
        },
        Expr::PodLayoutSizeOf { ty } => Expr::PodLayoutSizeOf {
            ty: substitute_type(ty, substitutions),
        },
        Expr::PodLayoutAlignOf { ty } => Expr::PodLayoutAlignOf {
            ty: substitute_type(ty, substitutions),
        },
        Expr::PodLayoutOffsetOf { ty, field_path } => Expr::PodLayoutOffsetOf {
            ty: substitute_type(ty, substitutions),
            field_path: field_path.clone(),
        },
        Expr::NativeArenaAlloc(size) => {
            Expr::NativeArenaAlloc(Box::new(substitute_expr(size, substitutions)))
        }
        Expr::NativeArenaView {
            owner,
            kind,
            byte_offset,
            length,
        } => Expr::NativeArenaView {
            owner: Box::new(substitute_expr(owner, substitutions)),
            kind: *kind,
            byte_offset: Box::new(substitute_expr(byte_offset, substitutions)),
            length: Box::new(substitute_expr(length, substitutions)),
        },
        Expr::NativePodView {
            owner,
            byte_offset,
            count,
            view_type,
        } => Expr::NativePodView {
            owner: Box::new(substitute_expr(owner, substitutions)),
            byte_offset: Box::new(substitute_expr(byte_offset, substitutions)),
            count: Box::new(substitute_expr(count, substitutions)),
            view_type: view_type
                .as_ref()
                .map(|ty| substitute_type(ty, substitutions)),
        },
        Expr::NativeArenaDispose(owner) => {
            Expr::NativeArenaDispose(Box::new(substitute_expr(owner, substitutions)))
        }
        Expr::NativeMemoryFillU32 { view, value } => Expr::NativeMemoryFillU32 {
            view: Box::new(substitute_expr(view, substitutions)),
            value: Box::new(substitute_expr(value, substitutions)),
        },
        Expr::NativeMemoryCopy { dst, src } => Expr::NativeMemoryCopy {
            dst: Box::new(substitute_expr(dst, substitutions)),
            src: Box::new(substitute_expr(src, substitutions)),
        },

        // References
        Expr::FuncRef(id) => Expr::FuncRef(*id),
        Expr::ExternFuncRef {
            name,
            param_types,
            return_type,
        } => Expr::ExternFuncRef {
            name: name.clone(),
            param_types: param_types.clone(),
            return_type: return_type.clone(),
        },
        Expr::NativeModuleRef(name) => Expr::NativeModuleRef(name.clone()),
        Expr::NativeMethodCall {
            module,
            class_name,
            object,
            method,
            args,
        } => Expr::NativeMethodCall {
            module: module.clone(),
            class_name: class_name.clone(),
            object: object
                .as_ref()
                .map(|o| Box::new(substitute_expr(o, substitutions))),
            method: method.clone(),
            args: args
                .iter()
                .map(|a| substitute_expr(a, substitutions))
                .collect(),
        },

        // Property access
        Expr::PropertyGet { object, property } => Expr::PropertyGet {
            object: Box::new(substitute_expr(object, substitutions)),
            property: property.clone(),
        },
        Expr::PropertySet {
            object,
            property,
            value,
        } => Expr::PropertySet {
            object: Box::new(substitute_expr(object, substitutions)),
            property: property.clone(),
            value: Box::new(substitute_expr(value, substitutions)),
        },
        Expr::PropertyUpdate {
            object,
            property,
            op,
            prefix,
        } => Expr::PropertyUpdate {
            object: Box::new(substitute_expr(object, substitutions)),
            property: property.clone(),
            op: *op,
            prefix: *prefix,
        },

        // Index access
        Expr::IndexGet { object, index } => Expr::IndexGet {
            object: Box::new(substitute_expr(object, substitutions)),
            index: Box::new(substitute_expr(index, substitutions)),
        },
        Expr::IndexSet {
            object,
            index,
            value,
        } => Expr::IndexSet {
            object: Box::new(substitute_expr(object, substitutions)),
            index: Box::new(substitute_expr(index, substitutions)),
            value: Box::new(substitute_expr(value, substitutions)),
        },

        // Literals
        Expr::Object(props) => Expr::Object(
            props
                .iter()
                .map(|(k, v)| (k.clone(), substitute_expr(v, substitutions)))
                .collect(),
        ),
        Expr::ObjectSpread { parts } => Expr::ObjectSpread {
            parts: parts
                .iter()
                .map(|(k, v)| (k.clone(), substitute_expr(v, substitutions)))
                .collect(),
        },
        Expr::Array(elems) => Expr::Array(
            elems
                .iter()
                .map(|e| substitute_expr(e, substitutions))
                .collect(),
        ),
        Expr::ArraySpread(elems) => Expr::ArraySpread(
            elems
                .iter()
                .map(|e| match e {
                    ArrayElement::Expr(expr) => {
                        ArrayElement::Expr(substitute_expr(expr, substitutions))
                    }
                    ArrayElement::Spread(expr) => {
                        ArrayElement::Spread(substitute_expr(expr, substitutions))
                    }
                    ArrayElement::Hole => ArrayElement::Hole,
                })
                .collect(),
        ),

        // Conditional
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => Expr::Conditional {
            condition: Box::new(substitute_expr(condition, substitutions)),
            then_expr: Box::new(substitute_expr(then_expr, substitutions)),
            else_expr: Box::new(substitute_expr(else_expr, substitutions)),
        },

        // Type operations
        Expr::TypeOf(inner) => Expr::TypeOf(Box::new(substitute_expr(inner, substitutions))),
        Expr::Void(inner) => Expr::Void(Box::new(substitute_expr(inner, substitutions))),
        Expr::Yield { value, delegate } => Expr::Yield {
            value: value
                .as_ref()
                .map(|v| Box::new(substitute_expr(v, substitutions))),
            delegate: *delegate,
        },
        Expr::InstanceOf { expr, ty, ty_expr } => Expr::InstanceOf {
            expr: Box::new(substitute_expr(expr, substitutions)),
            ty: ty.clone(),
            ty_expr: ty_expr
                .as_ref()
                .map(|e| Box::new(substitute_expr(e, substitutions))),
        },

        // Await
        Expr::Await(inner) => Expr::Await(Box::new(substitute_expr(inner, substitutions))),

        // New
        Expr::New {
            class_name,
            args,
            type_args,
            byte_offset,
            cap_args_appended,
        } => Expr::New {
            class_name: class_name.clone(),
            args: args
                .iter()
                .map(|a| substitute_expr(a, substitutions))
                .collect(),
            type_args: type_args
                .iter()
                .map(|t| substitute_type(t, substitutions))
                .collect(),
            byte_offset: *byte_offset,
            // Type substitution rewrites arg/type contents but preserves arg
            // COUNT and order, so the appended cap forwards stay trailing.
            cap_args_appended: *cap_args_appended,
        },

        // Class/Enum references
        Expr::ClassRef(name) => Expr::ClassRef(name.clone()),
        Expr::EnumMember {
            enum_name,
            member_name,
        } => Expr::EnumMember {
            enum_name: enum_name.clone(),
            member_name: member_name.clone(),
        },

        // Static field/method access
        Expr::StaticFieldGet {
            class_name,
            field_name,
        } => Expr::StaticFieldGet {
            class_name: class_name.clone(),
            field_name: field_name.clone(),
        },
        Expr::StaticFieldSet {
            class_name,
            field_name,
            value,
        } => Expr::StaticFieldSet {
            class_name: class_name.clone(),
            field_name: field_name.clone(),
            value: Box::new(substitute_expr(value, substitutions)),
        },
        Expr::StaticMethodCall {
            class_name,
            method_name,
            args,
        } => Expr::StaticMethodCall {
            class_name: class_name.clone(),
            method_name: method_name.clone(),
            args: args
                .iter()
                .map(|a| substitute_expr(a, substitutions))
                .collect(),
        },

        // This/Super
        Expr::This => Expr::This,
        Expr::SuperCall(args) => Expr::SuperCall(
            args.iter()
                .map(|a| substitute_expr(a, substitutions))
                .collect(),
        ),
        Expr::SuperMethodCall { method, args } => Expr::SuperMethodCall {
            method: method.clone(),
            args: args
                .iter()
                .map(|a| substitute_expr(a, substitutions))
                .collect(),
        },
        Expr::ObjectSuperPropertyGet {
            home,
            key,
            receiver,
        } => Expr::ObjectSuperPropertyGet {
            home: Box::new(substitute_expr(home, substitutions)),
            key: Box::new(substitute_expr(key, substitutions)),
            receiver: Box::new(substitute_expr(receiver, substitutions)),
        },
        Expr::SuperPropertySet {
            parent_class_id,
            parent_class_name,
            key,
            value,
        } => Expr::SuperPropertySet {
            parent_class_id: *parent_class_id,
            parent_class_name: parent_class_name.clone(),
            key: Box::new(substitute_expr(key, substitutions)),
            value: Box::new(substitute_expr(value, substitutions)),
        },
        Expr::ObjectSuperPropertySet {
            home,
            key,
            value,
            receiver,
        } => Expr::ObjectSuperPropertySet {
            home: Box::new(substitute_expr(home, substitutions)),
            key: Box::new(substitute_expr(key, substitutions)),
            value: Box::new(substitute_expr(value, substitutions)),
            receiver: Box::new(substitute_expr(receiver, substitutions)),
        },
        Expr::ObjectSuperMethodCall {
            home,
            key,
            receiver,
            args,
        } => Expr::ObjectSuperMethodCall {
            home: Box::new(substitute_expr(home, substitutions)),
            key: Box::new(substitute_expr(key, substitutions)),
            receiver: Box::new(substitute_expr(receiver, substitutions)),
            args: args
                .iter()
                .map(|a| substitute_expr(a, substitutions))
                .collect(),
        },

        // Environment
        Expr::EnvGet(name) => Expr::EnvGet(name.clone()),
        Expr::ProcessUptime => Expr::ProcessUptime,
        Expr::ProcessMemoryUsage => Expr::ProcessMemoryUsage,
        Expr::ProcessAbort => Expr::ProcessAbort,
        Expr::ProcessUmask(mask) => Expr::ProcessUmask(
            mask.as_ref()
                .map(|e| Box::new(substitute_expr(e, substitutions))),
        ),
        Expr::ProcessThreadCpuUsage(prior) => Expr::ProcessThreadCpuUsage(
            prior
                .as_ref()
                .map(|e| Box::new(substitute_expr(e, substitutions))),
        ),
        Expr::ProcessAvailableMemory => Expr::ProcessAvailableMemory,
        Expr::ProcessConstrainedMemory => Expr::ProcessConstrainedMemory,
        Expr::ProcessPosixCredential(k) => Expr::ProcessPosixCredential(*k),
        Expr::ProcessEmitWarning(args) => Expr::ProcessEmitWarning(
            args.iter()
                .map(|a| substitute_expr(a, substitutions))
                .collect(),
        ),
        Expr::ProcessCpuUsage(prior) => Expr::ProcessCpuUsage(
            prior
                .as_ref()
                .map(|e| Box::new(substitute_expr(e, substitutions))),
        ),
        Expr::ProcessResourceUsage => Expr::ProcessResourceUsage,
        Expr::ProcessActiveResourcesInfo => Expr::ProcessActiveResourcesInfo,
        Expr::ProcessHrtime(prior) => Expr::ProcessHrtime(
            prior
                .as_ref()
                .map(|e| Box::new(substitute_expr(e, substitutions))),
        ),
        Expr::ProcessTitle => Expr::ProcessTitle,
        Expr::ProcessSetTitle(v) => {
            Expr::ProcessSetTitle(Box::new(substitute_expr(v, substitutions)))
        }

        // File system
        Expr::FsReadFileSync(path) => {
            Expr::FsReadFileSync(Box::new(substitute_expr(path, substitutions)))
        }
        Expr::FsWriteFileSync(path, content) => Expr::FsWriteFileSync(
            Box::new(substitute_expr(path, substitutions)),
            Box::new(substitute_expr(content, substitutions)),
        ),
        Expr::FsExistsSync(path) => {
            Expr::FsExistsSync(Box::new(substitute_expr(path, substitutions)))
        }
        Expr::FsMkdirSync(path) => {
            Expr::FsMkdirSync(Box::new(substitute_expr(path, substitutions)))
        }
        Expr::FsUnlinkSync(path) => {
            Expr::FsUnlinkSync(Box::new(substitute_expr(path, substitutions)))
        }
        Expr::FsAppendFileSync(path, content) => Expr::FsAppendFileSync(
            Box::new(substitute_expr(path, substitutions)),
            Box::new(substitute_expr(content, substitutions)),
        ),

        // Path operations
        Expr::PathJoin(a, b) => Expr::PathJoin(
            Box::new(substitute_expr(a, substitutions)),
            Box::new(substitute_expr(b, substitutions)),
        ),
        Expr::PathWin32Join(a, b) => Expr::PathWin32Join(
            Box::new(substitute_expr(a, substitutions)),
            Box::new(substitute_expr(b, substitutions)),
        ),
        Expr::PathWin32 { method, args } => Expr::PathWin32 {
            method: *method,
            args: args
                .iter()
                .map(|e| substitute_expr(e, substitutions))
                .collect(),
        },
        Expr::PathDirname(path) => {
            Expr::PathDirname(Box::new(substitute_expr(path, substitutions)))
        }
        Expr::PathBasename(path) => {
            Expr::PathBasename(Box::new(substitute_expr(path, substitutions)))
        }
        Expr::PathExtname(path) => {
            Expr::PathExtname(Box::new(substitute_expr(path, substitutions)))
        }
        Expr::PathResolve(path) => {
            Expr::PathResolve(Box::new(substitute_expr(path, substitutions)))
        }
        Expr::PathIsAbsolute(path) => {
            Expr::PathIsAbsolute(Box::new(substitute_expr(path, substitutions)))
        }
        Expr::PathToNamespacedPath(path) => {
            Expr::PathToNamespacedPath(Box::new(substitute_expr(path, substitutions)))
        }
        Expr::PathMatchesGlob(a, b) => Expr::PathMatchesGlob(
            Box::new(substitute_expr(a, substitutions)),
            Box::new(substitute_expr(b, substitutions)),
        ),
        Expr::PathResolveJoin(a, b) => Expr::PathResolveJoin(
            Box::new(substitute_expr(a, substitutions)),
            Box::new(substitute_expr(b, substitutions)),
        ),

        // Array methods
        Expr::ArrayPush { array_id, value } => Expr::ArrayPush {
            array_id: *array_id,
            value: Box::new(substitute_expr(value, substitutions)),
        },
        Expr::ArrayPushSpread { array_id, source } => Expr::ArrayPushSpread {
            array_id: *array_id,
            source: Box::new(substitute_expr(source, substitutions)),
        },
        Expr::ArrayPop(id) => Expr::ArrayPop(*id),
        Expr::ArrayShift(id) => Expr::ArrayShift(*id),
        Expr::ArrayUnshift { array_id, value } => Expr::ArrayUnshift {
            array_id: *array_id,
            value: Box::new(substitute_expr(value, substitutions)),
        },
        Expr::ArrayIndexOf {
            array,
            value,
            from_index,
        } => Expr::ArrayIndexOf {
            array: Box::new(substitute_expr(array, substitutions)),
            value: Box::new(substitute_expr(value, substitutions)),
            from_index: from_index
                .as_ref()
                .map(|fi| Box::new(substitute_expr(fi, substitutions))),
        },
        Expr::ArrayLastIndexOf {
            array,
            value,
            from_index,
        } => Expr::ArrayLastIndexOf {
            array: Box::new(substitute_expr(array, substitutions)),
            value: Box::new(substitute_expr(value, substitutions)),
            from_index: from_index
                .as_ref()
                .map(|fi| Box::new(substitute_expr(fi, substitutions))),
        },
        Expr::ArrayIncludes {
            array,
            value,
            from_index,
        } => Expr::ArrayIncludes {
            array: Box::new(substitute_expr(array, substitutions)),
            value: Box::new(substitute_expr(value, substitutions)),
            from_index: from_index
                .as_ref()
                .map(|fi| Box::new(substitute_expr(fi, substitutions))),
        },
        Expr::ArraySlice { array, start, end } => Expr::ArraySlice {
            array: Box::new(substitute_expr(array, substitutions)),
            start: Box::new(substitute_expr(start, substitutions)),
            end: end
                .as_ref()
                .map(|e| Box::new(substitute_expr(e, substitutions))),
        },
        Expr::ArraySplice {
            array_id,
            start,
            delete_count,
            items,
        } => Expr::ArraySplice {
            array_id: *array_id,
            start: Box::new(substitute_expr(start, substitutions)),
            delete_count: delete_count
                .as_ref()
                .map(|d| Box::new(substitute_expr(d, substitutions))),
            items: items
                .iter()
                .map(|i| substitute_expr(i, substitutions))
                .collect(),
        },
        Expr::ArrayForEach { array, callback } => Expr::ArrayForEach {
            array: Box::new(substitute_expr(array, substitutions)),
            callback: Box::new(substitute_expr(callback, substitutions)),
        },
        Expr::ArrayMap { array, callback } => Expr::ArrayMap {
            array: Box::new(substitute_expr(array, substitutions)),
            callback: Box::new(substitute_expr(callback, substitutions)),
        },
        Expr::ArrayFilter { array, callback } => Expr::ArrayFilter {
            array: Box::new(substitute_expr(array, substitutions)),
            callback: Box::new(substitute_expr(callback, substitutions)),
        },
        Expr::ArrayFind { array, callback } => Expr::ArrayFind {
            array: Box::new(substitute_expr(array, substitutions)),
            callback: Box::new(substitute_expr(callback, substitutions)),
        },
        Expr::ArrayFindIndex { array, callback } => Expr::ArrayFindIndex {
            array: Box::new(substitute_expr(array, substitutions)),
            callback: Box::new(substitute_expr(callback, substitutions)),
        },
        Expr::ArraySort { array, comparator } => Expr::ArraySort {
            array: Box::new(substitute_expr(array, substitutions)),
            comparator: Box::new(substitute_expr(comparator, substitutions)),
        },
        Expr::ArrayReduce {
            array,
            callback,
            initial,
        } => Expr::ArrayReduce {
            array: Box::new(substitute_expr(array, substitutions)),
            callback: Box::new(substitute_expr(callback, substitutions)),
            initial: initial
                .as_ref()
                .map(|i| Box::new(substitute_expr(i, substitutions))),
        },
        Expr::ArrayReduceRight {
            array,
            callback,
            initial,
        } => Expr::ArrayReduceRight {
            array: Box::new(substitute_expr(array, substitutions)),
            callback: Box::new(substitute_expr(callback, substitutions)),
            initial: initial
                .as_ref()
                .map(|i| Box::new(substitute_expr(i, substitutions))),
        },
        Expr::ArrayJoin { array, separator } => Expr::ArrayJoin {
            array: Box::new(substitute_expr(array, substitutions)),
            separator: separator
                .as_ref()
                .map(|s| Box::new(substitute_expr(s, substitutions))),
        },
        Expr::ArrayLikeMethod {
            method,
            receiver,
            args,
        } => Expr::ArrayLikeMethod {
            method: method.clone(),
            receiver: Box::new(substitute_expr(receiver, substitutions)),
            args: args
                .iter()
                .map(|a| substitute_expr(a, substitutions))
                .collect(),
        },
        Expr::ArrayFlat { array } => Expr::ArrayFlat {
            array: Box::new(substitute_expr(array, substitutions)),
        },
        Expr::ArrayToReversed { array } => Expr::ArrayToReversed {
            array: Box::new(substitute_expr(array, substitutions)),
        },
        Expr::ArrayToSorted { array, comparator } => Expr::ArrayToSorted {
            array: Box::new(substitute_expr(array, substitutions)),
            comparator: comparator
                .as_ref()
                .map(|c| Box::new(substitute_expr(c, substitutions))),
        },
        Expr::ArrayToSpliced {
            array,
            start,
            delete_count,
            items,
        } => Expr::ArrayToSpliced {
            array: Box::new(substitute_expr(array, substitutions)),
            start: Box::new(substitute_expr(start, substitutions)),
            delete_count: Box::new(substitute_expr(delete_count, substitutions)),
            items: items
                .iter()
                .map(|i| substitute_expr(i, substitutions))
                .collect(),
        },
        Expr::ArrayWith {
            array,
            index,
            value,
        } => Expr::ArrayWith {
            array: Box::new(substitute_expr(array, substitutions)),
            index: Box::new(substitute_expr(index, substitutions)),
            value: Box::new(substitute_expr(value, substitutions)),
        },
        Expr::ArrayReverseValue { receiver } => Expr::ArrayReverseValue {
            receiver: Box::new(substitute_expr(receiver, substitutions)),
        },
        Expr::ArrayCopyWithin {
            array_id,
            target,
            start,
            end,
        } => Expr::ArrayCopyWithin {
            array_id: *array_id,
            target: Box::new(substitute_expr(target, substitutions)),
            start: Box::new(substitute_expr(start, substitutions)),
            end: end
                .as_ref()
                .map(|e| Box::new(substitute_expr(e, substitutions))),
        },
        Expr::ArrayCopyWithinValue {
            receiver,
            target,
            start,
            end,
        } => Expr::ArrayCopyWithinValue {
            receiver: Box::new(substitute_expr(receiver, substitutions)),
            target: Box::new(substitute_expr(target, substitutions)),
            start: Box::new(substitute_expr(start, substitutions)),
            end: end
                .as_ref()
                .map(|e| Box::new(substitute_expr(e, substitutions))),
        },
        Expr::ArrayEntries(array) => {
            Expr::ArrayEntries(Box::new(substitute_expr(array, substitutions)))
        }
        Expr::ArrayKeys(array) => Expr::ArrayKeys(Box::new(substitute_expr(array, substitutions))),
        Expr::ArrayValues(array) => {
            Expr::ArrayValues(Box::new(substitute_expr(array, substitutions)))
        }

        // String methods
        Expr::StringSplit(string, delimiter) => Expr::StringSplit(
            Box::new(substitute_expr(string, substitutions)),
            Box::new(substitute_expr(delimiter, substitutions)),
        ),
        Expr::StringFromCharCode(code) => {
            Expr::StringFromCharCode(Box::new(substitute_expr(code, substitutions)))
        }
        Expr::StringFromCharCodeSpread(code) => {
            Expr::StringFromCharCodeSpread(Box::new(substitute_expr(code, substitutions)))
        }

        // Map operations
        Expr::MapNew => Expr::MapNew,
        Expr::MapNewFromArray(expr) => {
            Expr::MapNewFromArray(Box::new(substitute_expr(expr, substitutions)))
        }
        Expr::MapSet { map, key, value } => Expr::MapSet {
            map: Box::new(substitute_expr(map, substitutions)),
            key: Box::new(substitute_expr(key, substitutions)),
            value: Box::new(substitute_expr(value, substitutions)),
        },
        Expr::MapGet { map, key } => Expr::MapGet {
            map: Box::new(substitute_expr(map, substitutions)),
            key: Box::new(substitute_expr(key, substitutions)),
        },
        Expr::MapHas { map, key } => Expr::MapHas {
            map: Box::new(substitute_expr(map, substitutions)),
            key: Box::new(substitute_expr(key, substitutions)),
        },
        Expr::MapDelete { map, key } => Expr::MapDelete {
            map: Box::new(substitute_expr(map, substitutions)),
            key: Box::new(substitute_expr(key, substitutions)),
        },
        Expr::MapSize(map) => Expr::MapSize(Box::new(substitute_expr(map, substitutions))),
        Expr::MapClear(map) => Expr::MapClear(Box::new(substitute_expr(map, substitutions))),
        Expr::MapEntries(map) => Expr::MapEntries(Box::new(substitute_expr(map, substitutions))),
        Expr::MapKeys(map) => Expr::MapKeys(Box::new(substitute_expr(map, substitutions))),
        Expr::MapValues(map) => Expr::MapValues(Box::new(substitute_expr(map, substitutions))),

        // Set operations
        Expr::SetNew => Expr::SetNew,
        Expr::SetNewFromArray(expr) => {
            Expr::SetNewFromArray(Box::new(substitute_expr(expr, substitutions)))
        }
        Expr::SetAdd { set_id, value } => Expr::SetAdd {
            set_id: *set_id,
            value: Box::new(substitute_expr(value, substitutions)),
        },
        Expr::SetHas { set, value } => Expr::SetHas {
            set: Box::new(substitute_expr(set, substitutions)),
            value: Box::new(substitute_expr(value, substitutions)),
        },
        Expr::SetDelete { set, value } => Expr::SetDelete {
            set: Box::new(substitute_expr(set, substitutions)),
            value: Box::new(substitute_expr(value, substitutions)),
        },
        Expr::SetSize(set) => Expr::SetSize(Box::new(substitute_expr(set, substitutions))),
        Expr::SetClear(set) => Expr::SetClear(Box::new(substitute_expr(set, substitutions))),
        Expr::SetValues(set) => Expr::SetValues(Box::new(substitute_expr(set, substitutions))),

        // JSON operations
        Expr::JsonParse(expr) => Expr::JsonParse(Box::new(substitute_expr(expr, substitutions))),
        Expr::JsonStringify(expr) => {
            Expr::JsonStringify(Box::new(substitute_expr(expr, substitutions)))
        }
        Expr::JsonRawJson(expr) => {
            Expr::JsonRawJson(Box::new(substitute_expr(expr, substitutions)))
        }
        Expr::JsonIsRawJson(expr) => {
            Expr::JsonIsRawJson(Box::new(substitute_expr(expr, substitutions)))
        }

        // Math operations
        Expr::MathFloor(expr) => Expr::MathFloor(Box::new(substitute_expr(expr, substitutions))),
        Expr::MathCeil(expr) => Expr::MathCeil(Box::new(substitute_expr(expr, substitutions))),
        Expr::MathRound(expr) => Expr::MathRound(Box::new(substitute_expr(expr, substitutions))),
        Expr::MathTrunc(expr) => Expr::MathTrunc(Box::new(substitute_expr(expr, substitutions))),
        Expr::MathSign(expr) => Expr::MathSign(Box::new(substitute_expr(expr, substitutions))),
        Expr::MathAbs(expr) => Expr::MathAbs(Box::new(substitute_expr(expr, substitutions))),
        Expr::MathSqrt(expr) => Expr::MathSqrt(Box::new(substitute_expr(expr, substitutions))),
        Expr::MathPow(base, exp) => Expr::MathPow(
            Box::new(substitute_expr(base, substitutions)),
            Box::new(substitute_expr(exp, substitutions)),
        ),
        Expr::MathImul(a, b) => Expr::MathImul(
            Box::new(substitute_expr(a, substitutions)),
            Box::new(substitute_expr(b, substitutions)),
        ),
        Expr::MathMin(args) => Expr::MathMin(
            args.iter()
                .map(|a| substitute_expr(a, substitutions))
                .collect(),
        ),
        Expr::MathMax(args) => Expr::MathMax(
            args.iter()
                .map(|a| substitute_expr(a, substitutions))
                .collect(),
        ),
        Expr::MathMinSpread(e) => Expr::MathMinSpread(Box::new(substitute_expr(e, substitutions))),
        Expr::MathMaxSpread(e) => Expr::MathMaxSpread(Box::new(substitute_expr(e, substitutions))),
        Expr::MathRandom => Expr::MathRandom,

        // Crypto operations
        Expr::CryptoRandomBytes(expr) => {
            Expr::CryptoRandomBytes(Box::new(substitute_expr(expr, substitutions)))
        }
        Expr::CryptoRandomUUID => Expr::CryptoRandomUUID,
        Expr::CryptoRandomUUIDv7 => Expr::CryptoRandomUUIDv7,
        Expr::CryptoSha256(expr) => {
            Expr::CryptoSha256(Box::new(substitute_expr(expr, substitutions)))
        }
        Expr::CryptoMd5(expr) => Expr::CryptoMd5(Box::new(substitute_expr(expr, substitutions))),

        // Date operations
        Expr::DateNow => Expr::DateNow,
        Expr::DateNew(args) => Expr::DateNew(
            args.iter()
                .map(|a| substitute_expr(a, substitutions))
                .collect(),
        ),
        Expr::DateGetTime(date) => {
            Expr::DateGetTime(Box::new(substitute_expr(date, substitutions)))
        }
        Expr::DateToISOString(date) => {
            Expr::DateToISOString(Box::new(substitute_expr(date, substitutions)))
        }
        Expr::DateGetFullYear(date) => {
            Expr::DateGetFullYear(Box::new(substitute_expr(date, substitutions)))
        }
        Expr::DateGetMonth(date) => {
            Expr::DateGetMonth(Box::new(substitute_expr(date, substitutions)))
        }
        Expr::DateGetDate(date) => {
            Expr::DateGetDate(Box::new(substitute_expr(date, substitutions)))
        }
        Expr::DateGetDay(date) => Expr::DateGetDay(Box::new(substitute_expr(date, substitutions))),
        Expr::DateGetHours(date) => {
            Expr::DateGetHours(Box::new(substitute_expr(date, substitutions)))
        }
        Expr::DateGetMinutes(date) => {
            Expr::DateGetMinutes(Box::new(substitute_expr(date, substitutions)))
        }
        Expr::DateGetSeconds(date) => {
            Expr::DateGetSeconds(Box::new(substitute_expr(date, substitutions)))
        }
        Expr::DateGetMilliseconds(date) => {
            Expr::DateGetMilliseconds(Box::new(substitute_expr(date, substitutions)))
        }

        // Sequence
        Expr::Sequence(exprs) => Expr::Sequence(
            exprs
                .iter()
                .map(|e| substitute_expr(e, substitutions))
                .collect(),
        ),

        // Closure
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
            func_id: *func_id,
            params: params
                .iter()
                .map(|p| Param {
                    id: p.id,
                    name: p.name.clone(),
                    ty: substitute_type(&p.ty, substitutions),
                    default: p
                        .default
                        .as_ref()
                        .map(|d| substitute_expr(d, substitutions)),
                    decorators: p.decorators.clone(),
                    is_rest: p.is_rest,
                    arguments_object: p.arguments_object.clone(),
                })
                .collect(),
            return_type: substitute_type(return_type, substitutions),
            body: substitute_stmts(body, substitutions),
            captures: captures.clone(),
            mutable_captures: mutable_captures.clone(),
            captures_this: *captures_this,
            captures_new_target: *captures_new_target,
            enclosing_class: enclosing_class.clone(),
            is_arrow: *is_arrow,
            is_async: *is_async,
            is_generator: *is_generator,
            is_strict: *is_strict,
        },

        // RegExp operations
        Expr::RegExp { pattern, flags } => Expr::RegExp {
            pattern: pattern.clone(),
            flags: flags.clone(),
        },
        Expr::RegExpTest { regex, string } => Expr::RegExpTest {
            regex: Box::new(substitute_expr(regex, substitutions)),
            string: Box::new(substitute_expr(string, substitutions)),
        },
        Expr::StringMatch { string, regex } => Expr::StringMatch {
            string: Box::new(substitute_expr(string, substitutions)),
            regex: Box::new(substitute_expr(regex, substitutions)),
        },
        Expr::StringReplace {
            string,
            pattern,
            replacement,
        } => Expr::StringReplace {
            string: Box::new(substitute_expr(string, substitutions)),
            pattern: Box::new(substitute_expr(pattern, substitutions)),
            replacement: Box::new(substitute_expr(replacement, substitutions)),
        },

        // Object.keys/values/entries
        Expr::ObjectKeys(obj) => Expr::ObjectKeys(Box::new(substitute_expr(obj, substitutions))),
        Expr::ForInKeys(obj) => Expr::ForInKeys(Box::new(substitute_expr(obj, substitutions))),
        Expr::ObjectValues(obj) => {
            Expr::ObjectValues(Box::new(substitute_expr(obj, substitutions)))
        }
        Expr::ObjectEntries(obj) => {
            Expr::ObjectEntries(Box::new(substitute_expr(obj, substitutions)))
        }

        // Array.isArray / Array.from
        Expr::ArrayIsArray(value) => {
            Expr::ArrayIsArray(Box::new(substitute_expr(value, substitutions)))
        }
        Expr::ArrayFrom(value) => Expr::ArrayFrom(Box::new(substitute_expr(value, substitutions))),
        Expr::ArrayFromArrayLikeHoley(value) => {
            Expr::ArrayFromArrayLikeHoley(Box::new(substitute_expr(value, substitutions)))
        }
        Expr::ArrayFromMapped {
            iterable,
            map_fn,
            this_arg,
        } => Expr::ArrayFromMapped {
            iterable: Box::new(substitute_expr(iterable, substitutions)),
            map_fn: Box::new(substitute_expr(map_fn, substitutions)),
            this_arg: this_arg
                .as_ref()
                .map(|t| Box::new(substitute_expr(t, substitutions))),
        },

        // Global built-in functions
        Expr::ParseInt { string, radix } => Expr::ParseInt {
            string: Box::new(substitute_expr(string, substitutions)),
            radix: radix
                .as_ref()
                .map(|r| Box::new(substitute_expr(r, substitutions))),
        },
        Expr::ParseFloat(string) => {
            Expr::ParseFloat(Box::new(substitute_expr(string, substitutions)))
        }
        Expr::NumberCoerce(value) => {
            Expr::NumberCoerce(Box::new(substitute_expr(value, substitutions)))
        }
        Expr::BigIntCoerce(value) => {
            Expr::BigIntCoerce(Box::new(substitute_expr(value, substitutions)))
        }
        Expr::StringCoerce(value) => {
            Expr::StringCoerce(Box::new(substitute_expr(value, substitutions)))
        }
        Expr::ObjectCoerce(value) => {
            Expr::ObjectCoerce(Box::new(substitute_expr(value, substitutions)))
        }
        Expr::IsNaN(value) => Expr::IsNaN(Box::new(substitute_expr(value, substitutions))),
        Expr::IsUndefinedOrBareNan(value) => {
            Expr::IsUndefinedOrBareNan(Box::new(substitute_expr(value, substitutions)))
        }
        Expr::IsFinite(value) => Expr::IsFinite(Box::new(substitute_expr(value, substitutions))),
        Expr::StaticPluginResolve(value) => {
            Expr::StaticPluginResolve(Box::new(substitute_expr(value, substitutions)))
        }
        // JS Runtime expressions - pass through unchanged (no type substitution needed)
        Expr::JsLoadModule { path } => Expr::JsLoadModule { path: path.clone() },
        Expr::JsGetExport {
            module_handle,
            export_name,
        } => Expr::JsGetExport {
            module_handle: Box::new(substitute_expr(module_handle, substitutions)),
            export_name: export_name.clone(),
        },
        Expr::JsCallFunction {
            module_handle,
            func_name,
            args,
        } => Expr::JsCallFunction {
            module_handle: Box::new(substitute_expr(module_handle, substitutions)),
            func_name: func_name.clone(),
            args: args
                .iter()
                .map(|a| substitute_expr(a, substitutions))
                .collect(),
        },
        Expr::JsCallMethod {
            object,
            method_name,
            args,
        } => Expr::JsCallMethod {
            object: Box::new(substitute_expr(object, substitutions)),
            method_name: method_name.clone(),
            args: args
                .iter()
                .map(|a| substitute_expr(a, substitutions))
                .collect(),
        },
        // OS module expressions - pass through unchanged
        Expr::OsPlatform => Expr::OsPlatform,
        Expr::OsArch => Expr::OsArch,
        Expr::OsHostname => Expr::OsHostname,
        Expr::OsType => Expr::OsType,
        Expr::OsRelease => Expr::OsRelease,
        Expr::OsHomedir => Expr::OsHomedir,
        Expr::OsTmpdir => Expr::OsTmpdir,
        Expr::OsTotalmem => Expr::OsTotalmem,
        Expr::OsFreemem => Expr::OsFreemem,
        Expr::OsCpus => Expr::OsCpus,
        Expr::OsNetworkInterfaces => Expr::OsNetworkInterfaces,
        Expr::OsUserInfo => Expr::OsUserInfo,
        Expr::OsUserInfoBuffer => Expr::OsUserInfoBuffer,
        Expr::OsUptime => Expr::OsUptime,
        Expr::OsEOL => Expr::OsEOL,
        Expr::OsDevNull => Expr::OsDevNull,
        Expr::OsAvailableParallelism => Expr::OsAvailableParallelism,
        Expr::OsEndianness => Expr::OsEndianness,
        Expr::OsLoadavg => Expr::OsLoadavg,
        Expr::OsMachine => Expr::OsMachine,
        Expr::OsVersion => Expr::OsVersion,
        // Catch-all for any other expressions that don't need type substitution
        _ => expr.clone(),
    }
}

/// Substitute types in statements
pub(crate) fn substitute_stmts(stmts: &[Stmt], substitutions: &HashMap<String, Type>) -> Vec<Stmt> {
    stmts
        .iter()
        .map(|stmt| substitute_stmt(stmt, substitutions))
        .collect()
}

/// Substitute types in a single statement
fn substitute_stmt(stmt: &Stmt, substitutions: &HashMap<String, Type>) -> Stmt {
    match stmt {
        Stmt::Let {
            id,
            name,
            ty,
            mutable,
            init,
        } => Stmt::Let {
            id: *id,
            name: name.clone(),
            ty: substitute_type(ty, substitutions),
            mutable: *mutable,
            init: init.as_ref().map(|e| substitute_expr(e, substitutions)),
        },
        Stmt::Expr(expr) => Stmt::Expr(substitute_expr(expr, substitutions)),
        Stmt::Return(expr) => {
            Stmt::Return(expr.as_ref().map(|e| substitute_expr(e, substitutions)))
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => Stmt::If {
            condition: substitute_expr(condition, substitutions),
            then_branch: substitute_stmts(then_branch, substitutions),
            else_branch: else_branch
                .as_ref()
                .map(|b| substitute_stmts(b, substitutions)),
        },
        Stmt::While { condition, body } => Stmt::While {
            condition: substitute_expr(condition, substitutions),
            body: substitute_stmts(body, substitutions),
        },
        Stmt::DoWhile { body, condition } => Stmt::DoWhile {
            body: substitute_stmts(body, substitutions),
            condition: substitute_expr(condition, substitutions),
        },
        Stmt::Labeled { label, body } => Stmt::Labeled {
            label: label.clone(),
            body: Box::new(substitute_stmt(body, substitutions)),
        },
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => Stmt::For {
            init: init
                .as_ref()
                .map(|s| Box::new(substitute_stmt(s, substitutions))),
            condition: condition
                .as_ref()
                .map(|e| substitute_expr(e, substitutions)),
            update: update.as_ref().map(|e| substitute_expr(e, substitutions)),
            body: substitute_stmts(body, substitutions),
        },
        Stmt::Break => Stmt::Break,
        Stmt::Continue => Stmt::Continue,
        Stmt::LabeledBreak(label) => Stmt::LabeledBreak(label.clone()),
        Stmt::LabeledContinue(label) => Stmt::LabeledContinue(label.clone()),
        Stmt::Throw(expr) => Stmt::Throw(substitute_expr(expr, substitutions)),
        Stmt::Try {
            body,
            catch,
            finally,
        } => Stmt::Try {
            body: substitute_stmts(body, substitutions),
            catch: catch.as_ref().map(|c| CatchClause {
                param: c.param.clone(),
                body: substitute_stmts(&c.body, substitutions),
            }),
            finally: finally.as_ref().map(|f| substitute_stmts(f, substitutions)),
        },
        Stmt::Switch {
            discriminant,
            cases,
        } => Stmt::Switch {
            discriminant: substitute_expr(discriminant, substitutions),
            cases: cases
                .iter()
                .map(|c| SwitchCase {
                    test: c.test.as_ref().map(|t| substitute_expr(t, substitutions)),
                    body: substitute_stmts(&c.body, substitutions),
                })
                .collect(),
        },
        Stmt::PreallocateBoxes(ids) => Stmt::PreallocateBoxes(ids.clone()),
        Stmt::PreallocateTdzBoxes(ids) => Stmt::PreallocateTdzBoxes(ids.clone()),
    }
}
