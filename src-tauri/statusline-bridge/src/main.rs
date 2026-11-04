fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(statusline_bridge::run::main_entry(&args));
}
