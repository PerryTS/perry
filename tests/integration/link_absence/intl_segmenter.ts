// HWS hook: js_runtime_install_intl_segmenter
// HWS prefixes: perry_runtime::intl::segmenter::|perry_runtime::intl::segments_view::
console.log(Array.from(new Intl.Segmenter("en", {granularity:"word"}).segment("hello world")).length);
