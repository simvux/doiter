use futures::{TryStreamExt, future::join_all};
use std::path::Path;

use bollard::{
    Docker,
    exec::{CreateExecOptions, StartExecOptions, StartExecResults},
};

pub struct Memory {
    counter: usize,
    magic: String,
    docker: Docker,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RootAddress(String);

impl RootAddress {
    pub fn bytes(&self) -> impl Iterator<Item = u8> {
        self.0.bytes()
    }
}

// We pin the version because otherwise we'll be constantly checking
// for image updates every single allocation.
//
// ... I may have discovered this by getting rate limited.
const IMAGE: &'static str = "docker:29.8";

impl Memory {
    pub async fn new() -> Result<Self, bollard::errors::Error> {
        let docker = Docker::connect_with_socket_defaults()?;

        let magic = {
            let params = bollard::query_parameters::ListContainersOptionsBuilder::new()
                .all(true)
                .build();

            let containers = docker.list_containers(Some(params)).await?;

            let mut i = 0;
            loop {
                if containers.iter().any(|container| {
                    container.names.as_ref().is_some_and(|names| {
                        names
                            .iter()
                            .any(|name| name.contains(&format!("doiter_magic_{i}__")))
                    })
                }) {
                    i += 1;
                    continue;
                }

                break format!("doiter_magic_{i}__");
            }
        };

        Ok(Self {
            magic,
            docker,
            counter: 0,
        })
    }

    pub fn from_raw_bytes(&self, bytes: &[u8]) -> Option<RootAddress> {
        str::from_utf8(bytes)
            .ok()
            .map(String::from)
            .map(RootAddress)
    }

    pub async fn alloc(&mut self) -> RootAddress {
        let counter = self.counter;
        self.counter += 1;

        let name = format!("{}{counter}", self.magic);

        // Fetch the docker image if it does not exist
        if !self
            .docker
            .list_images(None::<bollard::query_parameters::ListImagesOptions>)
            .await
            .unwrap()
            .iter()
            .any(|image| image.repo_tags.iter().any(|n| n == IMAGE))
        {
            self.docker
                .create_image(
                    Some(
                        bollard::query_parameters::CreateImageOptionsBuilder::default()
                            .from_image(IMAGE)
                            .build(),
                    ),
                    None,
                    None,
                )
                .try_collect::<Vec<_>>()
                .await
                .unwrap();
        }

        // Create the docker container
        let response = {
            let params = bollard::query_parameters::CreateContainerOptionsBuilder::new()
                .name(&name)
                .build();

            let config = bollard::config::ContainerCreateBody {
                cmd: Some(vec!["sleep".to_string(), "infinity".to_string()]),
                image: Some(IMAGE.to_string()),
                ..Default::default()
            };

            self.docker
                .create_container(Some(params), config)
                .await
                .unwrap()
        };

        for warning in &response.warnings {
            eprintln!("docker warning: {warning}");
        }

        self.docker.start_container(&name, None).await.unwrap();

        RootAddress(response.id)
    }

    pub async fn write(&mut self, addr: &RootAddress, path: &Path, data: &[u8]) {
        let data = data
            .iter()
            .map(|b| format!("\\x{b:02x}"))
            .collect::<String>();

        let command = format!(
            "printf '%b' '{}' >> {}",
            data.replace('\'', "\'"),
            path.display()
        );

        let exec = self.run_in_container(addr, &command).await;

        match exec {
            bollard::exec::StartExecResults::Detached => unreachable!(),
            bollard::exec::StartExecResults::Attached { output, .. } => {
                // NOTE: This is not just for debugging purposes. If we don't collect the output
                // then we don't block until the underlying `printf` command is complete.
                for log_output in output.try_collect::<Vec<_>>().await.unwrap() {
                    tracing::info!("{log_output}");
                }
            }
        }
    }

    async fn run_in_container(&mut self, addr: &RootAddress, command: &str) -> StartExecResults {
        let exec = self
            .docker
            .create_exec(
                &addr.0,
                CreateExecOptions {
                    cmd: Some(vec!["sh", "-c", command]),
                    attach_stdout: Some(true),
                    tty: Some(true),
                    privileged: Some(true),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        self.docker
            .start_exec(
                &exec.id,
                Some(StartExecOptions {
                    detach: false,
                    ..Default::default()
                }),
            )
            .await
            .unwrap()
    }

    pub async fn try_read(&mut self, addr: &RootAddress, path: &Path) -> Option<Vec<u8>> {
        let command = format!("cat {}", path.display());
        let exec = self.run_in_container(addr, &command).await;

        match exec {
            bollard::exec::StartExecResults::Detached => unreachable!(),
            bollard::exec::StartExecResults::Attached { output, .. } => {
                let mut buf = vec![];

                for log_output in output.try_collect::<Vec<_>>().await.unwrap() {
                    buf.extend_from_slice(log_output.as_ref());
                }

                // ... just pretend you didn't see this part
                let not_found = buf.starts_with(
                    &format!(
                        "cat: can't open '{}': No such file or directory",
                        path.display()
                    )
                    .as_bytes(),
                );

                if not_found { None } else { Some(buf) }
            }
        }
    }

    pub async fn cleanup_session(&mut self) {
        let magic = self.magic.clone();
        self.cleanup(Some(magic.as_str())).await
    }

    // TODO: refactor to use a `filter` parameters instead of blind Option
    pub async fn cleanup(&mut self, magic: Option<&str>) {
        let magic = magic.unwrap_or_else(|| "doiter_magic_".into());

        let params = bollard::query_parameters::ListContainersOptionsBuilder::new()
            .all(true)
            .build();

        let containers = match self.docker.list_containers(Some(params)).await {
            Ok(containers) => containers,
            Err(err) => {
                tracing::error!("Could not list containers for cleaup: {err}");
                return;
            }
        };

        let removals = containers.iter().map(|container| async {
            let should_remove = container
                .names
                .as_ref()
                .and_then(|names| names.iter().find(|name| name.contains(magic)));

            if let Some(name) = should_remove {
                // HACK: For some reason docker prefixes the names with `/` in the `list_containers` call
                // even though the containers don't have a slash in their name.
                let name = name.strip_prefix("/").unwrap_or(name.as_ref());

                if let Err(err) = self.docker.stop_container(name, None).await {
                    tracing::error!("Could not stop container {name}: {err}");
                }

                let params = bollard::query_parameters::WaitContainerOptionsBuilder::new()
                    .condition("not-running")
                    .build();

                if let Err(err) = self
                    .docker
                    .wait_container(name, Some(params))
                    .try_collect::<Vec<_>>()
                    .await
                {
                    tracing::warn!("Container {name} refused to stop: {err}");
                };

                let params = bollard::query_parameters::RemoveContainerOptionsBuilder::new()
                    .force(true)
                    .build();

                tracing::info!("Removing {name}...");
                let result = self.docker.remove_container(name, Some(params)).await;

                if let Err(err) = result {
                    eprintln!("Could not remove container {name}: {err}");
                }
            }
        });

        join_all(removals).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn read_write() {
        let mut mem = Memory::new().await.unwrap();

        let path_x = PathBuf::from("/x");
        let path_y = PathBuf::from("/y");

        let addr = mem.alloc().await;
        mem.write(&addr, &path_x, &[78]).await;
        mem.write(&addr, &path_y, &[79]).await;

        let bytes_x = mem.try_read(&addr, &path_x).await.unwrap();
        let bytes_y = mem.try_read(&addr, &path_y).await.unwrap();

        let magic = mem.magic.clone();
        mem.cleanup(Some(magic.as_str())).await;

        assert_eq!(bytes_x, vec![78]);
        assert_eq!(bytes_y, vec![79]);
    }

    #[tokio::test]
    async fn read_does_not_exist() {
        let mut mem = Memory::new().await.unwrap();

        let addr = mem.alloc().await;

        let path = PathBuf::from("/x");
        let load = mem.try_read(&addr, &path).await;

        let magic = mem.magic.clone();
        mem.cleanup(Some(magic.as_str())).await;

        assert_eq!(load, None);
    }
}
