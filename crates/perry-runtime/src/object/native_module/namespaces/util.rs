//! Item-level native namespace declarations for `util` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_UTIL_TYPES = "util.types" {
        cell = "node:sys.types";
        members = {
            isArgumentsObject = (ref("node:util/types.isArgumentsObject"));
            isArrayBuffer = (ref("node:util/types.isArrayBuffer"));
            isAsyncFunction = (ref("node:util/types.isAsyncFunction"));
            isBigIntObject = (ref("node:util/types.isBigIntObject"));
            isBooleanObject = (ref("node:util/types.isBooleanObject"));
            isDate = (ref("node:util/types.isDate"));
            isExternal = (ref("node:util/types.isExternal"));
            isGeneratorFunction = (ref("node:util/types.isGeneratorFunction"));
            isGeneratorObject = (ref("node:util/types.isGeneratorObject"));
            isMap = (ref("node:util/types.isMap"));
            isMapIterator = (ref("node:util/types.isMapIterator"));
            isModuleNamespaceObject = (ref("node:util/types.isModuleNamespaceObject"));
            isNativeError = (ref("node:util/types.isNativeError"));
            isNumberObject = (ref("node:util/types.isNumberObject"));
            isPromise = (ref("node:util/types.isPromise"));
            isProxy = (ref("node:util/types.isProxy"));
            isRegExp = (ref("node:util/types.isRegExp"));
            isSet = (ref("node:util/types.isSet"));
            isSetIterator = (ref("node:util/types.isSetIterator"));
            isSharedArrayBuffer = (ref("node:util/types.isSharedArrayBuffer"));
            isStringObject = (ref("node:util/types.isStringObject"));
            isSymbolObject = (ref("node:util/types.isSymbolObject"));
            isWeakMap = (ref("node:util/types.isWeakMap"));
            isWeakSet = (ref("node:util/types.isWeakSet"));
            isAnyArrayBuffer = (ref("node:util/types.isAnyArrayBuffer"));
            isBoxedPrimitive = (ref("node:util/types.isBoxedPrimitive"));
            isArrayBufferView = (ref("node:util/types.isArrayBufferView"));
            isDataView = (ref("node:util/types.isDataView"));
            isTypedArray = (ref("node:util/types.isTypedArray"));
            isUint8Array = (ref("node:util/types.isUint8Array"));
            isUint8ClampedArray = (ref("node:util/types.isUint8ClampedArray"));
            isUint16Array = (ref("node:util/types.isUint16Array"));
            isUint32Array = (ref("node:util/types.isUint32Array"));
            isInt8Array = (ref("node:util/types.isInt8Array"));
            isInt16Array = (ref("node:util/types.isInt16Array"));
            isInt32Array = (ref("node:util/types.isInt32Array"));
            isFloat16Array = (ref("node:util/types.isFloat16Array"));
            isFloat32Array = (ref("node:util/types.isFloat32Array"));
            isFloat64Array = (ref("node:util/types.isFloat64Array"));
            isBigInt64Array = (ref("node:util/types.isBigInt64Array"));
            isBigUint64Array = (ref("node:util/types.isBigUint64Array"));
            isKeyObject = (ref("node:util/types.isKeyObject"));
            isCryptoKey = (ref("node:util/types.isCryptoKey"));
        }
    }
}

crate::realm_decl! {
    namespace NM_UTIL_DEFAULT = "util.default" {
        cell = "node:sys.default";
        members = {
            aborted = (ref("node:sys.aborted"));
            callbackify = (ref("node:sys.callbackify"));
            convertProcessSignalToExitCode = (ref("node:sys.convertProcessSignalToExitCode"));
            debug;
            debuglog = (ref("node:sys.debug"));
            deprecate = (ref("node:sys.deprecate"));
            diff = (ref("node:sys.diff"));
            format = (ref("node:sys.format"));
            formatWithOptions = (ref("node:sys.formatWithOptions"));
            getCallSites = (ref("node:sys.getCallSites"));
            getSystemErrorMap = (ref("node:sys.getSystemErrorMap"));
            getSystemErrorName = (ref("node:sys.getSystemErrorName"));
            getSystemErrorMessage = (ref("node:sys.getSystemErrorMessage"));
            inherits = (ref("node:sys.inherits"));
            inspect = (ref("node:sys.inspect"));
            isArray = (ref("node:sys.isArray"));
            isDeepStrictEqual = (ref("node:sys.isDeepStrictEqual"));
            promisify = (ref("node:sys.promisify"));
            stripVTControlCharacters = (ref("node:sys.stripVTControlCharacters"));
            styleText = (ref("node:sys.styleText"));
            toUSVString = (ref("node:sys.toUSVString"));
            setTraceSigInt = (ref("node:sys.setTraceSigInt"));
            types = (ref("node:sys.types"));
            parseArgs = (ref("node:sys.parseArgs"));
            TextDecoder = (ref(TextDecoder));
            TextEncoder = (ref(TextEncoder));
            transferableAbortController = (ref("node:sys.transferableAbortController"));
            transferableAbortSignal = (ref("node:sys.transferableAbortSignal"));
        }
    }
}
