//! Machine-readable graph evidence for comparing the compatibility port.
//! cargo run --release -p perry-gc-effects --example inspect -- ARCHIVE...
fn main() -> anyhow::Result<()> {
    let paths = std::env::args_os()
        .skip(1)
        .map(std::path::PathBuf::from)
        .collect::<Vec<_>>();
    let g = perry_gc_effects::build_graph(&paths)?;
    let rows = g
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            serde_json::json!({
                "names":n.names,"obj":n.obj.as_ref(),"sec":n.sec.as_ref(),"addr":n.addr,"calls":n.calls,
                "externs":n.externs,"switches":n.switches,"idioms":n.idioms,"sites":n.sites,
                "label":g.label(i),"succ":g.succ[i]
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_writer(std::io::stdout().lock(), &rows)?;
    Ok(())
}
