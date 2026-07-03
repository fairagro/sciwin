use std::{
    io::{BufRead, BufReader},
    path::Path,
    process::{Child, Command},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use wait_timeout::ChildExt;

pub fn is_cwl_file(path: &str) -> bool {
    Path::new(path).extension().is_some_and(|ext| ext.eq_ignore_ascii_case("cwl"))
}

pub struct Output {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub fn handle_process(process: &mut Child, timelimit: u64) -> Result<Output, Box<dyn std::error::Error>> {
    let stdout_buf = Arc::new(Mutex::new(String::new()));
    let stderr_buf = Arc::new(Mutex::new(String::new()));

    let stdout = process.stdout.take().expect("Not Piped");
    let stderr = process.stderr.take().expect("Not Piped");

    let stdout_buf_clone = Arc::clone(&stdout_buf);
    let stdout_handle = thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = Vec::new();
        while reader.read_until(b'\n', &mut line).unwrap() > 0 {
            let line_str = String::from_utf8_lossy(&line);
            eprint!("{line_str}");
            stdout_buf_clone.lock().unwrap().push_str(&line_str);
            line.clear();
        }
    });

    let stderr_buf_clone = Arc::clone(&stderr_buf);
    let stderr_handle = thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut line = Vec::new();
        while reader.read_until(b'\n', &mut line).unwrap() > 0 {
            let line_str = String::from_utf8_lossy(&line);
            eprint!("{line_str}");
            stderr_buf_clone.lock().unwrap().push_str(&line_str);
            line.clear();
        }
    });

    let status = if timelimit > 0 {
        if process.wait_timeout(Duration::from_secs(timelimit))?.is_none() {
            process.kill()?;
            return Err("Time elapsed".into());
        }
        process.wait()?
    } else {
        process.wait()?
    };

    stdout_handle.join().unwrap();
    stderr_handle.join().unwrap();

    Ok(Output {
        stdout: Arc::try_unwrap(stdout_buf).unwrap().into_inner()?,
        stderr: Arc::try_unwrap(stderr_buf).unwrap().into_inner()?,
        exit_code: status.code().unwrap_or(1),
    })
}

pub fn is_docker_installed() -> bool {
    let output = Command::new("docker").arg("--version").output();

    matches!(output, Ok(output) if output.status.success())
}
