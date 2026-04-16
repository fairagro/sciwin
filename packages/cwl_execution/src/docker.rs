use crate::Result;
use crate::environment::RuntimeEnvironment;
use cwl_core::{Entry, StringOrNumber, requirements::DockerRequirement};
use rand::Rng;
use rand::distr::Alphanumeric;
use std::cell::RefCell;
use std::fmt::Display;
use std::process::{Command as SystemCommand, Stdio};
use std::{fs, path::MAIN_SEPARATOR_STR, process::Command};
use util::handle_process;

pub fn is_docker_installed() -> bool {
    let engine = container_engine().to_string();
    let output = Command::new(engine).arg("--version").output();

    matches!(output, Ok(output) if output.status.success())
}

#[derive(Default, Clone, Debug, Copy)]
pub enum ContainerEngine {
    #[default]
    Docker,
    Podman,
    Singularity,
    Apptainer,
}

impl Display for ContainerEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContainerEngine::Docker => write!(f, "docker"),
            ContainerEngine::Podman => write!(f, "podman"),
            ContainerEngine::Singularity => write!(f, "singularity"),
            ContainerEngine::Apptainer => write!(f, "apptainer"),
        }
    }
}

impl ContainerEngine {
    pub fn auto() -> Option<ContainerEngine> {
        if command_available("docker") {
            Some(ContainerEngine::Docker)
        } else if command_available("podman") {
            Some(ContainerEngine::Podman)
        } else if command_available("apptainer") {
            Some(ContainerEngine::Apptainer)
        } else if command_available("singularity") {
            Some(ContainerEngine::Singularity)
        } else {
            None
        }
    }
}

fn command_available(cmd: &str) -> bool {
    #[cfg(unix)]
    let checker = "which";

    #[cfg(windows)]
    let checker = "where";

    Command::new(checker).arg(cmd).output().map(|o| o.status.success()).unwrap_or(false)
}

pub fn configure_container_engine(engine: &Option<ContainerEngine>) {
    match engine {
        Some(ContainerEngine::Docker) => {
            set_container_engine(ContainerEngine::Docker);
        }
        Some(ContainerEngine::Podman) => {
            set_container_engine(ContainerEngine::Podman);
        }
        Some(ContainerEngine::Singularity) => {
            set_container_engine(ContainerEngine::Singularity);
        }
        Some(ContainerEngine::Apptainer) => {
            set_container_engine(ContainerEngine::Apptainer);
        }
        None => {
            log::info!("Running without container engine (native execution)");
        }
    }
}

thread_local! {static CONTAINER_ENGINE: RefCell<ContainerEngine> = const { RefCell::new(ContainerEngine::Docker) };}

pub fn set_container_engine(value: ContainerEngine) {
    CONTAINER_ENGINE.with(|engine| *engine.borrow_mut() = value);
}

pub fn container_engine() -> ContainerEngine {
    CONTAINER_ENGINE.with(|engine| *engine.borrow())
}

pub(crate) fn build_docker_command(command: &mut SystemCommand, docker: &DockerRequirement, runtime: &RuntimeEnvironment) -> Result<SystemCommand> {
    let container_engine = container_engine().to_string();
    let is_singularity = container_engine.contains("singularity") || container_engine.contains("apptainer");

    let docker_image = if let Some(pull) = &docker.docker_pull {
        pull
    } else if let (Some(docker_file), Some(docker_image_id)) = (&docker.docker_file, &docker.docker_image_id) {
        let path = match docker_file {
            Entry::Include(include) => include.include.clone(),
            Entry::Source(src) => {
                let path = format!("{}/Dockerfile", runtime.runtime["tmpdir"]);
                fs::write(&path, src)?;
                path
            }
        };
        let path = path.trim_start_matches(&("..".to_owned() + MAIN_SEPARATOR_STR)).to_string();

        let mut build = SystemCommand::new(&container_engine);
        let mut process = build
            .args(["build", "-f", &path, "-t", docker_image_id, "."])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        handle_process(&mut process, 0)?;
        docker_image_id
    } else {
        unreachable!()
    };
    let outdir = runtime
        .runtime
        .get("outdir")
        .map(|s| s.to_string())
        .unwrap_or_else(|| std::env::current_dir().unwrap().to_string_lossy().into_owned());

    let tmpdir = runtime.runtime.get("tmpdir").map(|s| s.to_string()).unwrap_or_else(|| "/tmp".to_string());

    let workdir = if let Some(docker_output_directory) = &docker.docker_output_directory {
        docker_output_directory
    } else {
        &format!("/{}", rand::rng().sample_iter(&Alphanumeric).take(5).map(char::from).collect::<String>())
    };
    let mut container_command = SystemCommand::new(&container_engine);

    if is_singularity {
        fs::create_dir_all("/tmp/apptainer_tmp")?;

        container_command.arg("exec");
        container_command.args([
            "-H",
            &format!("{outdir}:{workdir}"),
            "-B",
            "/tmp/apptainer_tmp:/tmp",
            "--pwd",
            workdir,
            "--env",
            "TMPDIR=/tmp",
        ]);
        container_command.arg(docker_image);
        container_command.arg(command.get_program());
    } else {
        let workdir_mount = format!("--mount=type=bind,source={outdir},target={workdir}");
        let tmpdir_mount = format!("--mount=type=bind,source={tmpdir},target=/tmp");
        let workdir_arg = format!("--workdir={}", &workdir);

        container_command.args(["run", "-i", &workdir_mount, &tmpdir_mount, &workdir_arg, "--rm"]);

        #[cfg(unix)]
        {
            container_command.arg(get_user_flag());
        }

        container_command.arg(format!("--env=HOME={}", &workdir));
        container_command.arg("--env=TMPDIR=/tmp");

        for (key, val) in command.get_envs().skip_while(|(key, _)| *key == "HOME" || *key == "TMPDIR") {
            container_command.arg(format!("--env={}={}", key.to_string_lossy(), val.unwrap().to_string_lossy()));
        }

        if let Some(StringOrNumber::Integer(i)) = runtime.runtime.get("network") {
            if *i != 1 {
                container_command.arg("--net=none");
            }
        } else {
            container_command.arg("--net=none");
        }

        container_command.arg(docker_image);
        container_command.arg(command.get_program());
    }

    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned().replace(&outdir, workdir).replace("\\", "/"))
        .collect::<Vec<_>>();
    container_command.args(args);

    container_command.stderr(Stdio::piped());
    container_command.stdout(Stdio::piped());

    Ok(container_command)
}

#[cfg(unix)]
fn get_user_flag() -> String {
    use nix::unistd::{getgid, getuid};
    format!("--user={}:{}", getuid().as_raw(), getgid().as_raw())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(target_os = "macos", ignore)]
    fn test_auto_container() {
        assert!(ContainerEngine::auto().is_some());
    }
}
