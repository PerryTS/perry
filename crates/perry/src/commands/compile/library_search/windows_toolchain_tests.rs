use super::msvc_vswhere_installation_path_args;

#[test]
fn vswhere_query_requires_msvc_tools_component() {
    assert_eq!(
        msvc_vswhere_installation_path_args(),
        [
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-latest",
            "-property",
            "installationPath",
            "-nologo",
        ]
    );
}
