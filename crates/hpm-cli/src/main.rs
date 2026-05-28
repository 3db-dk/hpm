use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    hpm_cli::run().await
}
