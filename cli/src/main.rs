use std::process::ExitCode;

fn main() -> ExitCode {
    runners_cli::command::run(
        std::env::args_os(),
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}
