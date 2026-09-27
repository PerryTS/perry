use super::*;

#[test]
fn computed_native_requires_survive_relocation_and_authenticate_payloads() {
    if !require_tool("clang") {
        return;
    }
    #[cfg(windows)]
    if !require_tool("llvm-dlltool") {
        return;
    }
    for variant in ["computed", "static-edge", "project", "platform-package"] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let install = temp.path().join("install");
        std::fs::create_dir_all(&install).unwrap();
        let package = source.join("node_modules/fixture-addon");
        std::fs::create_dir_all(&package).unwrap();
        let native = if variant == "project" {
            source.join("native")
        } else if variant == "platform-package" {
            source.join("node_modules/@parcel/watcher-test-platform")
        } else {
            package.join("build/Release")
        };
        std::fs::create_dir_all(&native).unwrap();
        let mut policy = serde_json::json!({
            "compilePackages": ["fixture-addon"],
            "allow": {"compilePackages": ["fixture-addon"]},
            "nativeAddons": ["fixture-addon"]
        });
        if variant == "project" {
            policy = serde_json::json!({"nativeAddonPaths": ["native/addon.node"]});
        }
        std::fs::write(
            source.join("package.json"),
            serde_json::json!({"private":true,"type":"module","perry":policy}).to_string(),
        )
        .unwrap();
        std::fs::write(package.join("package.json"), r#"{"name":"fixture-addon","version":"1.0.0","main":"index.js","optionalDependencies":{"@parcel/watcher-test-platform":"1.0.0"}}"#).unwrap();
        if variant == "platform-package" {
            std::fs::write(
                native.join("package.json"),
                r#"{"name":"@parcel/watcher-test-platform","version":"1.0.0","main":"addon.node"}"#,
            )
            .unwrap();
        }
        compile_addon(&source, &native);
        let wrapper = if variant == "platform-package" {
            "const name = '@parcel/watcher-test-platform'; const first = require(name); if (require(require.resolve(name)) !== first) throw new Error('platform resolve identity'); module.exports = first;"
                .to_string()
        } else {
            format!("{}\nconst path = require('path'); const filename = path.join(__dirname, 'build', 'Release', 'addon.node'); const first = require(filename); if (require(require.resolve(filename)) !== first) throw new Error('computed resolve identity'); module.exports = first;",
                if variant == "static-edge" { "if (process.env.PERRY_TEST_STATIC_EDGE) require('./build/Release/addon.node');" } else { "" })
        };
        std::fs::write(package.join("index.js"), wrapper).unwrap();
        let logical = if variant == "project" {
            "$project/native/addon.node"
        } else if variant == "platform-package" {
            "@parcel/watcher-test-platform/addon.node"
        } else {
            "fixture-addon/build/Release/addon.node"
        };
        let request = if variant == "project" {
            "./native/addon.node"
        } else if variant == "platform-package" {
            "@parcel/watcher-test-platform/addon.node"
        } else {
            "fixture-addon/build/Release/addon.node"
        };
        let entry = source.join("main.ts");
        std::fs::write(&entry, format!(r#"
import {{ createRequire }} from 'node:module';
{}
const load = createRequire(import.meta.url);
{}
const again = load({request:?});
const filename = load.resolve({request:?});
console.log('computed-addon', addon.answer, addon.add(19, 23), addon === again, load(filename) === addon, load.cache[filename].exports === addon);
if (process.argv[2]) {{
    try {{ load(process.argv[2]); console.log('UNAUTHORIZED LOAD'); }}
    catch (error) {{ console.log('rejected', error.code === 'ERR_DLOPEN_FAILED'); }}
}}
if (process.env.PERRY_TEST_DLOPEN) {{
    const direct = {{ exports: {{}} }};
    process.dlopen(direct, {logical:?});
    console.log('shared-cache', direct.exports === addon);
}}
"#, if variant == "project" { "" } else { "import addon from 'fixture-addon';" },
            if variant == "project" { "const addon = load('./native/addon.node');" } else { "" })).unwrap();
        let mut node = Command::new("node");
        node.arg(&entry);
        let oracle = run(node, "Node computed addon oracle");
        assert_eq!(
            String::from_utf8_lossy(&oracle.stdout),
            "computed-addon 8523 42 true true true\n"
        );
        let executable = install.join(if cfg!(windows) { "app.exe" } else { "app" });
        let output = compile_app(&source, &entry, &executable);
        assert!(
            output.status.success(),
            "{variant}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let sidecar = executable.with_file_name(format!(
            "{}.perry-native",
            executable.file_name().unwrap().to_string_lossy()
        ));
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(sidecar.join("manifest.json")).unwrap()).unwrap();
        let addons = manifest["addons"].as_array().unwrap();
        assert_eq!(
            addons.len(),
            1,
            "{variant}: manifest must contain the computed edge"
        );
        assert_eq!(addons[0]["logical_id"], logical);
        let rogue = temp.path().join("unlisted.node");
        std::fs::copy(native.join("addon.node"), &rogue).unwrap();
        std::fs::remove_dir_all(&source).unwrap();
        let output = run(Command::new(&executable), "relocated computed addon");
        assert_eq!(output.stdout, oracle.stdout, "{variant}");
        let mut authenticated = Command::new(&executable);
        authenticated.arg(&rogue).env("PERRY_TEST_DLOPEN", "1");
        let output = run(authenticated, "addon authorization and shared cache");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "computed-addon 8523 42 true true true\nrejected true\nshared-cache true\n",
            "{variant}"
        );
        let payload = sidecar.join(addons[0]["entry"].as_str().unwrap());
        let mut bytes = std::fs::read(&payload).unwrap();
        bytes[0] ^= 1;
        std::fs::write(&payload, bytes).unwrap();
        let output = Command::new(&executable).output().unwrap();
        assert!(
            !output.status.success(),
            "{variant}: corrupted addon was accepted"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("ERR_DLOPEN_FAILED"),
            "{variant}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
