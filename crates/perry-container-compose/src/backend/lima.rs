use super::*;
use crate::error::Result;
use crate::types::{
    ComposeNetwork, ComposeServiceBuild, ComposeVolume, ContainerInfo, ContainerSpec, ImageInfo,
};
use std::collections::HashMap;

pub struct LimaProtocol {
    pub instance: String,
}

impl CliProtocol for LimaProtocol {
    fn capabilities(&self) -> &'static crate::capabilities::BackendCapabilities {
        &crate::capabilities::BackendCapabilities::LIMA
    }

    fn run_args(&self, spec: &ContainerSpec) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.run_args(spec));
        args
    }
    fn create_args(&self, spec: &ContainerSpec) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.create_args(spec));
        args
    }
    fn start_args(&self, id: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.start_args(id));
        args
    }
    fn stop_args(&self, id: &str, timeout: Option<u32>) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.stop_args(id, timeout));
        args
    }
    fn remove_args(&self, id: &str, force: bool) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.remove_args(id, force));
        args
    }
    fn list_args(&self, all: bool) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.list_args(all));
        args
    }
    fn inspect_args(&self, id: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.inspect_args(id));
        args
    }
    fn logs_args(&self, id: &str, tail: Option<u32>) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.logs_args(id, tail));
        args
    }
    fn exec_args(
        &self,
        id: &str,
        cmd: &[String],
        env: Option<&HashMap<String, String>>,
        workdir: Option<&str>,
    ) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.exec_args(id, cmd, env, workdir));
        args
    }
    fn pull_image_args(&self, reference: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.pull_image_args(reference));
        args
    }
    fn list_images_args(&self) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.list_images_args());
        args
    }
    fn remove_image_args(&self, reference: &str, force: bool) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.remove_image_args(reference, force));
        args
    }
    fn create_network_args(&self, name: &str, config: &ComposeNetwork) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.create_network_args(name, config));
        args
    }
    fn remove_network_args(&self, name: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.remove_network_args(name));
        args
    }
    fn create_volume_args(&self, name: &str, config: &ComposeVolume) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.create_volume_args(name, config));
        args
    }
    fn remove_volume_args(&self, name: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.remove_volume_args(name));
        args
    }
    fn inspect_network_args(&self, name: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.inspect_network_args(name));
        args
    }
    fn inspect_volume_args(&self, name: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.inspect_volume_args(name));
        args
    }
    fn inspect_image_args(&self, reference: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.inspect_image_args(reference));
        args
    }
    fn build_args(&self, spec: &ComposeServiceBuild, image_name: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.build_args(spec, image_name));
        args
    }
    fn security_args(&self, profile: &SecurityProfile) -> Vec<String> {
        // Return only the nerdctl flags, the caller (run_with_security) will insert them
        // into the already prefixed run_args.
        DockerProtocol.security_args(profile)
    }
    fn parse_list_output(&self, stdout: &str) -> Result<Vec<ContainerInfo>> {
        DockerProtocol.parse_list_output(stdout)
    }
    fn parse_inspect_output(&self, stdout: &str) -> Result<ContainerInfo> {
        DockerProtocol.parse_inspect_output(stdout)
    }
    fn parse_list_images_output(&self, stdout: &str) -> Result<Vec<ImageInfo>> {
        DockerProtocol.parse_list_images_output(stdout)
    }
    fn parse_container_id(&self, stdout: &str) -> Result<String> {
        DockerProtocol.parse_container_id(stdout)
    }
}
