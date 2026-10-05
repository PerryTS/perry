use super::*;

pub(super) const NODE_DOMAIN_ROWS: &[NativeModSig] = &[
    NativeModSig {
        module: "domain",
        has_receiver: false,
        method: "Domain",
        class_filter: None,
        runtime: "js_domain_create",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "domain",
        has_receiver: false,
        method: "createDomain",
        class_filter: None,
        runtime: "js_domain_create",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "domain",
        has_receiver: false,
        method: "create",
        class_filter: None,
        runtime: "js_domain_create",
        args: &[],
        ret: NR_F64,
    },
];
