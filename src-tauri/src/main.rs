// Scheduled ticks must not flash a console, including local/debug installs.
// File logging and explicitly redirected stdout/stderr remain available.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match claude_switchboard_lib::cli::parse_args(&args) {
        claude_switchboard_lib::cli::CliMode::Tick => {
            // Initialize logging for headless tick.
            let override_dir =
                claude_switchboard_lib::cli::tick_data_dir(&args).unwrap_or_else(|e| {
                    eprintln!("{e:#}");
                    std::process::exit(2);
                });
            let log_dir = override_dir
                .as_ref()
                .map(|p| p.join("logs"))
                .unwrap_or_else(claude_switchboard_lib::logging::log_dir);
            let _log_guard = claude_switchboard_lib::logging::init(log_dir);

            let rt = tokio::runtime::Runtime::new().expect("tokio rt");
            let data_dir = override_dir.unwrap_or_else(claude_switchboard_lib::store::default_dir);
            if let Err(e) = rt.block_on(claude_switchboard_lib::cli::run_tick(&data_dir)) {
                eprintln!("--tick failed: {e:#}");
                std::process::exit(1);
            }
        }
        claude_switchboard_lib::cli::CliMode::RegisterScheduler => {
            if let Err(e) = claude_switchboard_lib::cli::register_scheduler() {
                eprintln!("Scheduler registration failed: {e:#}");
                std::process::exit(1);
            }
        }
        claude_switchboard_lib::cli::CliMode::Migrate
        | claude_switchboard_lib::cli::CliMode::Gui => {
            claude_switchboard_lib::run();
        }
    }
}
