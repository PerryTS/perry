use anyhow::{bail, Context, Result};
use pretty_assertions::assert_eq;
use semver::Version;
use serde::Deserialize;
use std::{
    collections::HashMap,
    ffi::OsStr,
    fs::{self, File},
    path::{Path, PathBuf},
};
use wac_graph::{types::Package, CompositionGraph, EncodeOptions, NodeId, PackageId};
use wit_component::{ComponentEncoder, StringEncoding};
use wit_parser::Resolve;

/// Represents a node to add to a composition graph.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
enum Node {
    Import {
        name: String,
        package: usize,
        export: String,
    },
    Instantiation {
        package: usize,
    },
    Alias {
        source: usize,
        export: String,
    },
}

/// Represents an argument to connect to an instantiation node.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Argument {
    source: usize,
    target: usize,
    name: String,
}

/// Represents a package (in WAT) to parse and register with a composition
/// graph.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WatPackage {
    name: String,
    version: Option<Version>,
    path: PathBuf,
}

/// Represents a name to associate with a node in a composition graph.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Name {
    node: usize,
    name: String,
}

/// Represents a node to export from a composition graph.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Export {
    node: usize,
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphFile {
    /// A list of packages to parse and register with the graph.
    #[serde(default)]
    packages: Vec<WatPackage>,
    /// A list of nodes to add to the graph.
    #[serde(default)]
    nodes: Vec<Node>,
    /// A list of nodes to connect to instantiation nodes.
    #[serde(default)]
    arguments: Vec<Argument>,
    /// A list of names to apply to nodes in the graph.
    #[serde(default)]
    names: Vec<Name>,
    /// A list of nodes to export.
    #[serde(default)]
    exports: Vec<Export>,
}

impl GraphFile {
    pub fn into_composition_graph(self, root: &Path, test_case: &str) -> Result<CompositionGraph> {
        let mut graph = CompositionGraph::new();
        let packages = self.register_packages(root, test_case, &mut graph)?;
        let nodes = self.add_nodes(test_case, &packages, &mut graph)?;
        self.add_arguments(test_case, &nodes, &mut graph)?;

        for (index, export) in self.exports.iter().enumerate() {
            let id = nodes
                .get(&export.node)
                .with_context(|| format!("invalid node index {export} referenced in export {index} for test case `{test_case}`", export = export.node))
                .copied()?;

            graph.export(id, &export.name).with_context(|| {
                format!(
                    "failed to export node {node} in export {index} for test case `{test_case}`",
                    node = export.node
                )
            })?;
        }

        for (index, name) in self.names.iter().enumerate() {
            let id = nodes
                .get(&name.node)
                .with_context(|| format!("invalid node index {node} referenced in name {index} for test case `{test_case}`", node = name.node))
                .copied()?;

            graph.set_node_name(id, &name.name);
        }

        Ok(graph)
    }

    fn load_wit_package(test_case: &str, path: &Path) -> Result<Vec<u8>> {
        let mut resolve = Resolve::default();
        let (id, _) = resolve.push_path(path).with_context(|| {
            format!(
                "failed to parse package file `{path}` for test case `{test_case}`",
                path = path.display()
            )
        })?;
        let world = resolve.select_world(&[id], None).with_context(|| {
            format!(
                "failed to select world from `{path}` for test case `{test_case}`",
                path = path.display()
            )
        })?;

        let mut module = wit_component::dummy_module(
            &resolve,
            world,
            wit_parser::ManglingAndAbi::Legacy(wit_parser::LiftLowerAbi::Sync),
        );
        wit_component::embed_component_metadata(
            &mut module,
            &resolve,
            world,
            StringEncoding::default(),
        )
        .with_context(|| {
            format!(
            "failed to embed component metadata from package `{path}` for test case `{test_case}`",
            path = path.display()
        )
        })?;

        let mut encoder = ComponentEncoder::default();
        encoder.validate(true).module(&module)?;
        encoder
            .encode()
            .with_context(|| format!("failed to encode a component from module derived from package `{path}` for test case `{test_case}`", path = path.display()))
    }

    fn register_packages(
        &self,
        root: &Path,
        test_case: &str,
        graph: &mut CompositionGraph,
    ) -> Result<HashMap<usize, PackageId>> {
        let mut packages = HashMap::new();

        for (index, package) in self.packages.iter().enumerate() {
            let path = root.join(&package.path);
            let bytes = match path.extension().and_then(OsStr::to_str) {
                Some("wit") => Self::load_wit_package(test_case, &path)?,
                Some("wat") => wat::parse_file(&path).with_context(|| {
                    format!(
                        "failed to parse package `{path}` for test case `{test_case}`",
                        path = path.display()
                    )
                })?,
                None if path.is_dir() => Self::load_wit_package(test_case, &path)?,
                _ => bail!(
                    "unexpected file extension for package file `{path}`",
                    path = package.path.display()
                ),
            };

            let package = Package::from_bytes(
                &package.name,
                package.version.as_ref(),
                bytes,
                graph.types_mut(),
            )
            .with_context(|| {
                format!(
                    "failed to decode package `{path}` for test case `{test_case}`",
                    path = path.display()
                )
            })?;

            let id = graph.register_package(package).with_context(|| {
                format!(
                    "failed to register package `{path}` for test case `{test_case}`",
                    path = path.display()
                )
            })?;

            packages.insert(index, id);
        }

        Ok(packages)
    }

    fn add_nodes(
        &self,
        test_case: &str,
        packages: &HashMap<usize, PackageId>,
        graph: &mut CompositionGraph,
    ) -> Result<HashMap<usize, NodeId>> {
        let mut nodes = HashMap::new();
        for (index, node) in self.nodes.iter().enumerate() {
            let id = match node {
                Node::Import {
                    name,
                    package,
                    export,
                } => {
                    let id = packages.get(package).with_context(|| {
                        format!("invalid package index {package} referenced in node {index} for test case `{test_case}`")
                    }).copied()?;
                    let kind = graph.types()[graph[id].ty()].exports.get(export).copied().with_context(|| {
                        format!("invalid package export `{export}` referenced in node {index} for test case `{test_case}`")
                    })?.promote();
                    graph.import(name, kind).with_context(|| {
                        format!("failed to add import node {index} for test case `{test_case}`")
                    })?
                }
                Node::Instantiation { package } => {
                    let package = packages
                        .get(package)
                        .with_context(|| {
                            format!("invalid package index {package} referenced in node {index} for test case `{test_case}`")
                        })
                        .copied()?;

                    graph.instantiate(package)
                }
                Node::Alias { source, export } => {
                    let source = nodes.get(source).with_context(|| {
                        format!("invalid source node index {source} referenced in node {index} for test case `{test_case}`")
                    })
                    .copied()?;

                    graph
                        .alias_instance_export(source, export)
                        .with_context(|| {
                            format!("failed to add alias node {index} for test case `{test_case}`")
                        })?
                }
            };

            nodes.insert(index, id);
        }

        Ok(nodes)
    }

    fn add_arguments(
        &self,
        test_case: &str,
        nodes: &HashMap<usize, NodeId>,
        graph: &mut CompositionGraph,
    ) -> Result<()> {
        for (index, argument) in self.arguments.iter().enumerate() {
            let source = nodes.get(&argument.source).with_context(|| {
                format!("invalid source node index {source} referenced in argument {index} for test case `{test_case}`", source = argument.source)
            }).copied()?;

            let target = nodes.get(&argument.target).with_context(|| {
                format!("invalid target node index {target} referenced in argument {index} for test case `{test_case}`", target = argument.target)
            }).copied()?;

            graph.set_instantiation_argument(target, &argument.name, source).with_context(|| {
                format!("failed to add argument edge from source node {source} to target node {target} referenced in argument {index} for test case `{test_case}`", source = argument.source, target = argument.target)
            })?;
        }

        Ok(())
    }
}

/// Tests the encoding of composition graphs.
///
/// This test looks in the `graphs/` directory for test cases.
///
/// The expected input files for a test case are:
/// * [required] `graph.json` - a JSON representation of a composition graph
///   (see above for serialization format).
/// * [optional] `*.wat` - packages (i.e. components) referenced from `graph.json`.
/// * [optional] `*.wit` - packages (i.e. components) referenced from `graph.json`;
///   the file is expected to contain a single world representing the world of
///   the component; a dummy module will be created to implement the component.
///
/// And the output files are one of the following:
///
/// * `encoded.wat` - the encoded component if the encoding is expected to succeed.
/// * `error.txt` - the expected error message if the encoding is expected to fail.
///
/// The test builds a composition graph from the JSON representation and attempts to
/// encode it. If the encoding succeeds, it expects the output to match `encoded.wat`.
/// If the encoding fails, it expects the output to match `error.txt`.
///
/// Run the test with the environment variable `BLESS` set to update
/// either `encoded.wat` or `error.txt` depending on the outcome of the encoding.
#[test]
fn encoding() -> Result<()> {
    for entry in fs::read_dir("tests/graphs")? {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }

        let test_case = path.file_stem().unwrap().to_str().unwrap();
        println!("================ test: {test_case} ================");
        let graph = path.join("graph.json");
        let output_path = path.join("encoded.wat");
        let error_path = path.join("error.txt");

        let file = File::open(&graph).with_context(|| {
            format!("failed to read graph file `{path}`", path = graph.display())
        })?;

        let r = serde_json::from_reader::<_, GraphFile>(file)
            .with_context(|| {
                format!(
                    "failed to deserialize graph file `{path}` for test case `{test_case}`",
                    path = graph.display()
                )
            })?
            .into_composition_graph(&path, test_case)
            .and_then(|graph| {
                println!("{:?}", graph);
                graph
                    .encode(EncodeOptions {
                        // We import the component definitions instead
                        // of defining them inline to make the encoded wat
                        // more readable and to test encoding a bit more.
                        define_components: false,
                        validate: true,
                        ..Default::default()
                    })
                    .context("failed to encode the graph")
            });

        let (output, baseline_path) = if error_path.is_file() {
            match r {
                Ok(_) => bail!("expected a test failure for test case `{test_case}`"),
                Err(e) => (format!("{e:?}\n").replace('\\', "/"), &error_path),
            }
        } else {
            let bytes =
                r.with_context(|| format!("failed to encode for test case `{test_case}`"))?;

            (
                wasmprinter::print_bytes(&bytes).with_context(|| {
                    format!("failed to print encoded bytes for test case `{test_case}`")
                })?,
                &output_path,
            )
        };

        if std::env::var_os("BLESS").is_some() {
            fs::write(baseline_path, output)?;
        } else {
            assert_eq!(
                fs::read_to_string(baseline_path)
                    .with_context(|| format!(
                        "failed to read test baseline `{path}`",
                        path = baseline_path.display()
                    ))?
                    .replace("\r\n", "\n"),
                output,
                "failed baseline comparison for test case `{test_case}` ({path})",
                path = baseline_path.display(),
            );
        }
    }

    Ok(())
}
