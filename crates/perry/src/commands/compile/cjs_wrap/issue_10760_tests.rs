#[test]
fn circular_warning_scan_distinguishes_deferred_reads_from_module_init() {
    let source = "const a = require('./a');\n\
                  function later() { return a.later; }\n\
                  if (true) { a.now; }\n\
                  (function () { a.immediate; })();\n";
    let masked = super::detect::strip_comments_and_strings(source);
    let sites: Vec<usize> = ["a.later", "a.now", "a.immediate"]
        .iter()
        .map(|needle| masked.find(needle).unwrap())
        .collect();
    let deferred = super::extract_requires::deferred_function_sites(&masked, &sites);
    assert_eq!(deferred.len(), 1);
    assert!(deferred.contains(&sites[0]));
}
