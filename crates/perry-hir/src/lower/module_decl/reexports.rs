use crate::ir::{Import, ImportSpecifier};

pub(crate) fn imported_binding_reexport(
    imports: &[Import],
    local: &str,
) -> Option<(String, String)> {
    imports.iter().find_map(|import| {
        if import.is_native {
            return None;
        }
        import
            .specifiers
            .iter()
            .find_map(|specifier| match specifier {
                ImportSpecifier::Named {
                    imported,
                    local: imported_local,
                } if imported_local == local => Some((import.source.clone(), imported.clone())),
                ImportSpecifier::Default {
                    local: imported_local,
                } if imported_local == local => {
                    Some((import.source.clone(), "default".to_string()))
                }
                ImportSpecifier::Namespace { .. } => None,
                _ => None,
            })
    })
}
