//! Print analyzer phase costs without executing a runtime or changing tables.
use perry_gc_effects::*;
fn main() -> anyhow::Result<()> {
    let paths = std::env::args_os()
        .skip(1)
        .map(std::path::PathBuf::from)
        .collect::<Vec<_>>();
    let start = std::time::Instant::now();
    let rules = load_rules(SEEDS)?;
    let rules_time = start.elapsed();
    let start = std::time::Instant::now();
    let g = build_graph(&paths)?;
    let graph_time = start.elapsed();
    let start = std::time::Instant::now();
    let res = classify(&g, &rules, Some(&workspace_root()))?;
    let classify_time = start.elapsed();
    let start = std::time::Instant::now();
    let shadow = shadowed_symbols(&workspace_root())?;
    println!("rules={rules_time:?} graph={graph_time:?} classify={classify_time:?} shadow={:?} nodes={} rows={} shadow_names={}",start.elapsed(),g.nodes.len(),res.table.len(),shadow.len());
    Ok(())
}
