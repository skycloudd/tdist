use crate::task_file::{TaskFile, TaskFileCommand};
use std::sync::atomic::{AtomicI32, Ordering};
use tracing::{info, warn};

pub struct Task {
    pub id: i32,
    pub name: String,
    repeat: usize,
    commands: Vec<Command>,
}

impl Task {
    pub fn from_task_file(task_file: TaskFile, task_id: &AtomicI32) -> Self {
        Self {
            id: task_id.fetch_add(1, Ordering::SeqCst),
            name: task_file.name,
            repeat: task_file.repeat.0,
            commands: task_file.commands.into_iter().map(Into::into).collect(),
        }
    }

    pub fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut repeat = self.repeat;
        let is_infinite = repeat == 0;

        while is_infinite || repeat > 0 {
            self.run_commands()?;

            repeat = repeat.saturating_sub(1);
        }

        Ok(())
    }

    fn run_commands(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut handles = Vec::new();

        for command in &self.commands {
            match command {
                Command::Shell {
                    command,
                    ignore_failure,
                    parallel,
                } => {
                    if *parallel {
                        info!("Running command in parallel: {}", command);

                        let handle = std::thread::spawn({
                            let command = command.clone();
                            let ignore_failure = *ignore_failure;

                            move || {
                                let status = std::process::Command::new("sh")
                                    .arg("-c")
                                    .arg(&command)
                                    .status()
                                    .map_err(|err| err.to_string())?;

                                info!("Finished running command: {}", command);

                                if ignore_failure {
                                    warn!("Ignoring failure for command: {}", command);

                                    Ok(())
                                } else if !status.success() {
                                    Err(format!("Command `{}` failed: {}", command, status))
                                } else {
                                    Ok(())
                                }
                            }
                        });

                        handles.push(handle);
                    } else {
                        info!("Running command: {}", command);

                        let status = std::process::Command::new("sh")
                            .arg("-c")
                            .arg(command)
                            .status()?;

                        info!("Finished running command: {}", command);

                        if *ignore_failure {
                            warn!("Ignoring failure for command: {}", command);
                        } else if !status.success() {
                            return Err(format!("Command `{}` failed: {}", command, status).into());
                        }

                        for handle in handles.drain(..) {
                            handle.join().unwrap()?;
                        }
                    }
                }
            }
        }

        for handle in handles.drain(..) {
            handle.join().unwrap()?;
        }

        Ok(())
    }
}

pub enum Command {
    Shell {
        command: String,
        ignore_failure: bool,
        parallel: bool,
    },
}

impl From<TaskFileCommand> for Command {
    fn from(taskfile_command: TaskFileCommand) -> Self {
        if let Some(command) = taskfile_command.shell {
            Self::Shell {
                command,
                ignore_failure: taskfile_command.ignore_failure,
                parallel: taskfile_command.parallel,
            }
        } else {
            panic!()
        }
    }
}
