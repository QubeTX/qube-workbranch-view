use clap::Parser;
use wb300::cli::Cli;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    let cli = Cli::parse();
    wb300::run(cli).await
}
