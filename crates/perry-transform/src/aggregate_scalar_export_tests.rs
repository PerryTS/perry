use super::*;

#[test]
fn exported_aggregate_keeps_materialized_carrier() {
    for metadata_source in ["exports", "exported_objects"] {
        let mut module = tests::aggregate_fixture(false);
        if metadata_source == "exports" {
            module.exports.push(Export::Named {
                local: "values".to_string(),
                exported: "VALUES".to_string(),
            });
        } else {
            module.exported_objects.push("values".to_string());
        }

        run(&mut module);

        assert!(
            module.init.iter().any(|stmt| {
                matches!(
                    stmt,
                    Stmt::Let {
                        id: 1,
                        init: Some(Expr::Array(_)),
                        ..
                    }
                )
            }),
            "{metadata_source} must keep the exported carrier"
        );
        assert!(
            module
                .init
                .iter()
                .any(|stmt| matches!(stmt, Stmt::Let { id: 2, .. })),
            "{metadata_source} must keep element aliases"
        );
    }
}
