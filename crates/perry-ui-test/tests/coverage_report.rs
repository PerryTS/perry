use perry_ui_test::{features_by_category, Support, CATEGORY_ORDER, FEATURES};

#[test]
fn coverage_matrix() {
    if std::env::var("PERRY_PRINT_MATRIX").is_err() {
        eprintln!("Skipped. Run with PERRY_PRINT_MATRIX=1 to print the feature matrix.");
        return;
    }

    let header = format!(
        "{:<50} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5}",
        "Function", "macOS", "iOS", "Andrd", "GTK4", "Win", "Web"
    );
    let separator = "─".repeat(header.len());

    println!();
    println!("{}", header);
    println!("{}", separator);

    for &cat in CATEGORY_ORDER {
        let feats = features_by_category(cat);
        if feats.is_empty() {
            continue;
        }
        println!("--- {} ---", cat);
        for f in &feats {
            println!(
                "{:<50} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5}",
                f.name,
                support_char(f.macos),
                support_char(f.ios),
                support_char(f.android),
                support_char(f.gtk4),
                support_char(f.windows),
                support_char(f.web),
            );
        }
    }

    println!("{}", separator);
    println!();

    // Summary
    let total = FEATURES.len();
    let platforms: &[(&str, fn(&perry_ui_test::Feature) -> Support)] = &[
        ("macOS", |f| f.macos),
        ("iOS", |f| f.ios),
        ("Android", |f| f.android),
        ("GTK4", |f| f.gtk4),
        ("Windows", |f| f.windows),
        ("Web", |f| f.web),
    ];

    println!("Summary ({} total features):", total);
    for &(name, get_support) in platforms {
        let supported = FEATURES
            .iter()
            .filter(|f| {
                let s = get_support(f);
                s == Support::Supported || s == Support::Stub
            })
            .count();
        println!(
            "  {:<10} {:>3}/{} ({:.0}%)",
            name,
            supported,
            total,
            supported as f64 / total as f64 * 100.0
        );
    }
    println!();
}

fn support_char(s: Support) -> &'static str {
    match s {
        Support::Supported => "Y",
        Support::Stub => "~",
        Support::Unsupported => "-",
    }
}
