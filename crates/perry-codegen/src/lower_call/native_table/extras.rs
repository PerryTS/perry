use super::*;

pub(super) const EXTRAS_ROWS: &[NativeModSig] = &[
    // ========== worker_threads ==========
    NativeModSig {
        module: "worker_threads",
        has_receiver: false,
        method: "getEnvironmentData",
        class_filter: None,
        runtime: "js_worker_threads_get_environment_data",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "worker_threads",
        has_receiver: false,
        method: "setEnvironmentData",
        class_filter: None,
        runtime: "js_worker_threads_set_environment_data",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "worker_threads",
        has_receiver: false,
        method: "getWorkerData",
        class_filter: None,
        runtime: "js_worker_threads_get_worker_data",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "worker_threads",
        has_receiver: false,
        method: "workerData",
        class_filter: None,
        runtime: "js_worker_threads_get_worker_data",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "worker_threads",
        has_receiver: false,
        method: "parentPort",
        class_filter: None,
        runtime: "js_worker_threads_parent_port",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "worker_threads",
        has_receiver: true,
        method: "postMessage",
        class_filter: None,
        runtime: "js_worker_threads_post_message",
        args: &[NA_F64],
        ret: NR_F64,
    },
    // ========== ethers ==========
    // Utility functions (receiver-less, no class filter).
    NativeModSig {
        module: "ethers",
        has_receiver: false,
        method: "getAddress",
        class_filter: None,
        runtime: "js_ethers_get_address",
        args: &[NA_STR],
        ret: NR_STR,
    },
    NativeModSig {
        module: "ethers",
        has_receiver: false,
        method: "formatEther",
        class_filter: None,
        runtime: "js_ethers_format_ether",
        args: &[NA_PTR],
        ret: NR_STR,
    },
    NativeModSig {
        module: "ethers",
        has_receiver: false,
        method: "formatUnits",
        class_filter: None,
        runtime: "js_ethers_format_units",
        args: &[NA_PTR, NA_F64],
        ret: NR_STR,
    },
    NativeModSig {
        module: "ethers",
        has_receiver: false,
        method: "parseEther",
        class_filter: None,
        runtime: "js_ethers_parse_ether",
        args: &[NA_STR],
        ret: NR_BIGINT,
    },
    NativeModSig {
        module: "ethers",
        has_receiver: false,
        method: "parseUnits",
        class_filter: None,
        runtime: "js_ethers_parse_units",
        args: &[NA_STR, NA_F64],
        ret: NR_BIGINT,
    },
    // Wallet.createRandom() — static method on the Wallet class.
    // class_filter matches `Wallet` so `ethers.Wallet.createRandom()` in
    // HIR (which lowers to class_name="Wallet", method="createRandom")
    // resolves here.
    NativeModSig {
        module: "ethers",
        has_receiver: false,
        method: "createRandom",
        class_filter: Some("Wallet"),
        runtime: "js_ethers_wallet_create_random",
        args: &[],
        ret: NR_PTR,
    },
];
