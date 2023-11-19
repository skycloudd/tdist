use crate::task::Task;
use crate::task_file::{get_task_files, TaskFile};
use clap::{Parser, Subcommand};
use crossbeam::deque::{Steal, Stealer, Worker};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicI32, Ordering},
    thread::JoinHandle,
};
use tracing::{error, info, warn};

mod task;
mod task_file;

#[derive(Parser)]
struct Args {
    #[clap(subcommand)]
    command: ArgCommand,
}

#[derive(Subcommand)]
enum ArgCommand {
    Run {
        #[clap(short, long)]
        config: Option<PathBuf>,
    },
    Config {
        #[clap(short, long)]
        config: Option<PathBuf>,
    },
}

#[derive(Serialize, Deserialize)]
struct Config {
    threads: usize,
    log_level: String,
    taskfile_dir: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            threads: 4,
            log_level: String::from("info"),
            taskfile_dir: PathBuf::from("taskfiles"),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    match args.command {
        ArgCommand::Run { config } => main_run(config),
        ArgCommand::Config { config } => edit_config(config),
    }
}

fn edit_config(config: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| String::from("vi"));

    let config_path = match config {
        Some(path) => path,
        None => confy::get_configuration_file_path("tdist", Some("config"))?,
    };

    let mut command = std::process::Command::new(editor)
        .arg(config_path)
        .spawn()?;

    let status = command.wait()?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("Editor failed: {}", status).into())
    }
}

fn main_run(config: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let config: Config = match config {
        Some(path) => confy::load_path(path),
        None => confy::load("tdist", Some("config")),
    }
    .map_err(|err| {
        format!(
            "Loading config from '{}': {}",
            confy::get_configuration_file_path("tdist", Some("config"))
                .unwrap()
                .to_string_lossy(),
            err
        )
    })?;

    let max_level = match config.log_level.as_str().to_lowercase().as_str() {
        "trace" => tracing::Level::TRACE,
        "debug" => tracing::Level::DEBUG,
        "info" => tracing::Level::INFO,
        "warn" => tracing::Level::WARN,
        "error" => tracing::Level::ERROR,
        _ => return Err(format!("Invalid log level: {}", config.log_level).into()),
    };

    tracing_subscriber::fmt()
        .with_max_level(max_level)
        .with_thread_ids(true)
        .init();

    info!(
        "Loaded config from '{}'",
        confy::get_configuration_file_path("tdist", Some("config"))?.to_string_lossy()
    );

    run(config).map_err(|err| {
        error!("Fatal error: {}", err);
        std::process::exit(1);
    })
}

fn run(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let worker = Worker::<Task>::new_fifo();

    let stealer = worker.stealer();

    for id in 0..config.threads {
        let thread_name = format!("worker-{}", id);

        match start_worker_thread(thread_name.clone(), stealer.clone()) {
            Ok(_) => info!("Started {}", thread_name),
            Err(error) => error!("Starting {}: {}", thread_name, error),
        }
    }

    let task_dir = Path::new(&config.taskfile_dir);

    info!(
        "Reading tasks from directory '{}'",
        task_dir.to_string_lossy()
    );

    let files = get_task_files(task_dir)?;

    let next_task_id = AtomicI32::new(0);

    for file in files {
        let file_content = std::fs::read_to_string(&file)?;
        let task_file: TaskFile = toml::from_str(&file_content)?;

        let task = Task::from_task_file(task_file, &next_task_id);

        info!("Creating task {}: {}", task.id, task.name);

        worker.push(task);
    }

    if next_task_id.load(Ordering::SeqCst) == 0 {
        warn!("No tasks found");
    }

    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

fn start_worker_thread(
    thread_name: String,
    stealer: Stealer<Task>,
) -> std::io::Result<JoinHandle<()>> {
    std::thread::Builder::new()
        .name(thread_name.clone())
        .spawn(|| task_stealer(stealer))
}

fn task_stealer(stealer: Stealer<Task>) {
    let backoff = crossbeam::utils::Backoff::new();
    let mut should_print_warning = true;

    loop {
        let task = stealer.steal();

        match task {
            Steal::Empty => {
                if should_print_warning {
                    warn!("Waiting for new tasks");
                    should_print_warning = false;
                }

                backoff.spin()
            }
            Steal::Success(task) => {
                info!("Running task {}: {}", task.id, task.name);

                if let Err(error) = task.run() {
                    error!("{}", error);
                }

                info!("Finished task {}", task.id);

                backoff.reset();
                should_print_warning = true;
            }
            Steal::Retry => {
                warn!("Retrying");

                continue;
            }
        }
    }
}
