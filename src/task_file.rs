use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub fn get_task_files<P: AsRef<Path>>(task_dir: P) -> std::io::Result<Vec<PathBuf>> {
    std::fs::read_dir(task_dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
}

#[derive(Serialize, Deserialize)]
pub struct TaskFile {
    pub name: String,
    #[serde(default)]
    pub repeat: Repeat,
    #[serde(rename = "command")]
    pub commands: Vec<TaskFileCommand>,
}

#[derive(Serialize, Deserialize)]
pub struct Repeat(pub usize);

impl Default for Repeat {
    fn default() -> Self {
        Self(1)
    }
}

#[derive(Serialize, Deserialize)]
pub struct TaskFileCommand {
    pub shell: Option<String>,

    #[serde(default)]
    pub ignore_failure: bool,

    #[serde(default)]
    pub parallel: bool,
}
