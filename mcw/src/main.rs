#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    let result = mcw::command::run(std::env::args_os().skip(1).collect());
    if let Err(error) = &result {
        mcw::platform::attach_console();
        eprintln!("mcw: {error}");
    }
    std::process::exit(result.unwrap_or(1));
}
