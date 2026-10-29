//! Shared link-name and typed birth mechanism for native function-info cells.
//! Runtime and provider crates supply their closure-header ABI type; the body,
//! parameter count and builders remain at the owning declaration.

#[macro_export]
macro_rules! fn_info {
    (@image_name $body:path, $n:tt, [$($builder:ident($($arg:tt)*)),*]) => {
        concat!(
            "perry_fn_info_", env!("CARGO_PKG_NAME"), "::", stringify!($body),
            "::", stringify!($n), "::",
            $(stringify!($builder), "(", $crate::fn_info!(@image_tokens $($arg)*), ");",)*
        )
    };
    (for $header:ty; reference $($rest:tt)*) => {{
        // The owning/imported record is immutable static storage.
        unsafe { &*$crate::fn_info!(for $header; $($rest)*) }
    }};
    (for $header:ty; import $body:path, $n:tt $(; $($m:ident($($a:tt)*)),* $(,)?)?) => {{
        unsafe extern "C" {
            #[link_name = $crate::fn_info!(@image_name $body, $n, [$($($m($($a)*)),*)?])]
            static INFO: $crate::JsFunctionInfo;
        }
        ::core::ptr::addr_of!(INFO)
    }};

    // An owning site opts its existing static into external linkage. Repeated
    // allocations reuse that site's pointer; they do not define another export.
    // The spelling derives only from the body and its ABI builder tokens, never
    // a source line, registry index, or manually assigned symbol table.
    // Concatenate lexical tokens recursively, including delimiters. Unlike
    // stringify!(an expression), this spelling does not depend on whitespace
    // chosen by Rust's expression pretty-printer. The source producer uses the
    // same token stream, so flag unions and builder calls need no symbol table.
    (@image_tokens $($token:tt)*) => {
        concat!($($crate::fn_info!(@image_token $token), " ",)*)
    };
    (@image_token ($($token:tt)*)) => {
        concat!("( ", $crate::fn_info!(@image_tokens $($token)*), ")")
    };
    (@image_token [$($token:tt)*]) => {
        concat!("[ ", $crate::fn_info!(@image_tokens $($token)*), "]")
    };
    (@image_token {$($token:tt)*}) => {
        concat!("{ ", $crate::fn_info!(@image_tokens $($token)*), "}")
    };
    (@image_token $token:tt) => { stringify!($token) };
    (@image_static $vis:vis $name:ident, $body:path, $n:tt, [$($builder:ident($($arg:tt)*)),*], $init:expr) => {
        #[export_name = $crate::fn_info!(@image_name $body, $n, [$($builder($($arg)*)),*])]
        $vis static $name: $crate::JsFunctionInfo = $init;
    };
    (@image_record plain $body:path, $n:tt, [$($builder:ident($($arg:tt)*)),*], $init:expr) => {{
        $crate::fn_info!(@image_static INFO, $body, $n, [$($builder($($arg)*)),*], $init);
        &INFO as *const $crate::JsFunctionInfo
    }};
    // A runtime-native record owns its canonical metadata entry, exactly as
    // the runtime's unexported records do; the export names the same storage.
    (@image_record owned $body:path, $n:tt, [$($builder:ident($($arg:tt)*)),*], $init:expr) => {{
        static OWNER: $crate::FunctionMetadataOwner = $crate::FunctionMetadataOwner::native();
        #[export_name = $crate::fn_info!(@image_name $body, $n, [$($builder($($arg)*)),*])]
        static INFO: $crate::NativeFunctionInfo = $crate::NativeFunctionInfo::new($init, &OWNER);
        &INFO.info as *const $crate::JsFunctionInfo
    }};
    (for $header:ty; export static $vis:vis $name:ident = sentinel $body:path, $n:tt $(; $($m:ident($($a:tt)*)),* $(,)?)?) => {
        $crate::fn_info!(@image_static $vis $name, $body, $n, [$($($m($($a)*)),*)?],
            unsafe { $crate::JsFunctionInfo::from_code($body, $n) } $($(.$m($($a)*))*)?);
    };
    (for $header:ty; export static $vis:vis $name:ident = native_args $body:path, $n:tt $(; $($m:ident($($a:tt)*)),* $(,)?)?) => {
        $crate::fn_info!(@image_static $vis $name, $body, $n, [$($($m($($a)*)),*)?],
            $crate::JsFunctionInfo::of_native_args(
                $body as $crate::JsNativeArgsBody<$header>, $n
            ) $($(.$m($($a)*))*)?);
    };
    (for $header:ty; export static $vis:vis $name:ident = $body:path, $n:tt $(; $($m:ident($($a:tt)*)),* $(,)?)?) => {
        $crate::fn_info!(@image_static $vis $name, $body, $n, [$($($m($($a)*)),*)?],
            $crate::JsFunctionInfo::of($body as $crate::fn_info!(@ty $header, $n)) $($(.$m($($a)*))*)?);
    };
    // Expression forms: providers' records are plain; the runtime asks for
    // records that own their metadata entry (`owned for ...`).
    (for $header:ty; export $($rest:tt)*) => { $crate::fn_info!(@export plain $header; $($rest)*) };
    (owned for $header:ty; export static $($rest:tt)*) => { $crate::fn_info! { for $header; export static $($rest)* } };
    (owned for $header:ty; export $($rest:tt)*) => { $crate::fn_info!(@export owned $header; $($rest)*) };
    (owned for $header:ty; reference $($rest:tt)*) => {{
        unsafe { &*$crate::fn_info!(owned for $header; $($rest)*) }
    }};
    (owned for $header:ty; $($rest:tt)*) => { $crate::fn_info!(for $header; $($rest)*) };
    (@export $mode:ident $header:ty; native_args $body:path, $declared:tt $(; $($m:ident($($a:tt)*)),* $(,)?)?) => {{
        $crate::fn_info!(@image_record $mode $body, $declared,
            [$($($m($($a)*)),*)?],
            $crate::JsFunctionInfo::of_native_args(
                $body as $crate::JsNativeArgsBody<$header>,
                $declared,
            ) $($(.$m($($a)*))*)?
        )
    }};
    (@export $mode:ident $header:ty; unwind_in_tests $body:path, $n:tt $(; $($m:ident($($a:tt)*)),* $(,)?)?) => {{
        #[cfg(panic = "abort")]
        let info = $crate::fn_info!(@export $mode $header; $body, $n $(; $($m($($a)*)),*)?);
        #[cfg(not(panic = "abort"))]
        let info = $crate::fn_info!(@export $mode $header; unwind $body, $n $(; $($m($($a)*)),*)?);
        info
    }};
    (@export $mode:ident $header:ty; unwind $body:path, $n:tt $(; $($m:ident($($a:tt)*)),* $(,)?)?) => {{
        $crate::fn_info!(@image_record $mode $body, $n,
            [$($($m($($a)*)),*)?],
            $crate::JsFunctionInfo::of($body as $crate::fn_info!(@uty $header, $n))
                $($(.$m($($a)*))*)?
        )
    }};
    (@export $mode:ident $header:ty; $body:path, $n:tt $(; $($m:ident($($a:tt)*)),* $(,)?)?) => {{
        $crate::fn_info!(@image_record $mode $body, $n,
            [$($($m($($a)*)),*)?],
            $crate::JsFunctionInfo::of($body as $crate::fn_info!(@ty $header, $n))
                $($(.$m($($a)*))*)?
        )
    }};
    (@ty $header:ty, 0) => { $crate::JsBody0<$header> };
    (@ty $header:ty, 1) => { $crate::JsBody1<$header> };
    (@ty $header:ty, 2) => { $crate::JsBody2<$header> };
    (@ty $header:ty, 3) => { $crate::JsBody3<$header> };
    (@ty $header:ty, 4) => { $crate::JsBody4<$header> };
    (@ty $header:ty, 5) => { $crate::JsBody5<$header> };
    (@ty $header:ty, 6) => { $crate::JsBody6<$header> };
    (@ty $header:ty, 7) => { $crate::JsBody7<$header> };
    (@ty $header:ty, 8) => { $crate::JsBody8<$header> };
    (@ty $header:ty, 9) => { $crate::JsBody9<$header> };
    (@ty $header:ty, 10) => { $crate::JsBody10<$header> };
    (@ty $header:ty, 11) => { $crate::JsBody11<$header> };
    (@ty $header:ty, 12) => { $crate::JsBody12<$header> };
    (@ty $header:ty, 13) => { $crate::JsBody13<$header> };
    (@ty $header:ty, 14) => { $crate::JsBody14<$header> };
    (@ty $header:ty, 15) => { $crate::JsBody15<$header> };
    (@ty $header:ty, 16) => { $crate::JsBody16<$header> };
    (@ty $header:ty, 17) => { $crate::JsBody17<$header> };
    (@ty $header:ty, 18) => { $crate::JsBody18<$header> };
    (@ty $header:ty, 19) => { $crate::JsBody19<$header> };
    (@ty $header:ty, 20) => { $crate::JsBody20<$header> };
    (@ty $header:ty, 21) => { $crate::JsBody21<$header> };
    (@ty $header:ty, 22) => { $crate::JsBody22<$header> };
    (@ty $header:ty, 23) => { $crate::JsBody23<$header> };
    (@ty $header:ty, 24) => { $crate::JsBody24<$header> };
    (@ty $header:ty, 25) => { $crate::JsBody25<$header> };
    (@ty $header:ty, 26) => { $crate::JsBody26<$header> };
    (@ty $header:ty, 27) => { $crate::JsBody27<$header> };
    (@ty $header:ty, 28) => { $crate::JsBody28<$header> };
    (@ty $header:ty, 29) => { $crate::JsBody29<$header> };
    (@ty $header:ty, 30) => { $crate::JsBody30<$header> };
    (@ty $header:ty, 31) => { $crate::JsBody31<$header> };
    (@ty $header:ty, 32) => { $crate::JsBody32<$header> };
    (@uty $header:ty, 0) => { $crate::JsBodyUnwind0<$header> };
    (@uty $header:ty, 1) => { $crate::JsBodyUnwind1<$header> };
    (@uty $header:ty, 2) => { $crate::JsBodyUnwind2<$header> };
    (@uty $header:ty, 3) => { $crate::JsBodyUnwind3<$header> };
    (@uty $header:ty, 4) => { $crate::JsBodyUnwind4<$header> };
    (@uty $header:ty, 5) => { $crate::JsBodyUnwind5<$header> };
    (@uty $header:ty, 6) => { $crate::JsBodyUnwind6<$header> };
    (@uty $header:ty, 7) => { $crate::JsBodyUnwind7<$header> };
    (@uty $header:ty, 8) => { $crate::JsBodyUnwind8<$header> };
    (@uty $header:ty, 9) => { $crate::JsBodyUnwind9<$header> };
    (@uty $header:ty, 10) => { $crate::JsBodyUnwind10<$header> };
    (@uty $header:ty, 11) => { $crate::JsBodyUnwind11<$header> };
    (@uty $header:ty, 12) => { $crate::JsBodyUnwind12<$header> };
    (@uty $header:ty, 13) => { $crate::JsBodyUnwind13<$header> };
    (@uty $header:ty, 14) => { $crate::JsBodyUnwind14<$header> };
    (@uty $header:ty, 15) => { $crate::JsBodyUnwind15<$header> };
    (@uty $header:ty, 16) => { $crate::JsBodyUnwind16<$header> };
}
