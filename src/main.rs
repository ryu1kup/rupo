use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "rupo",
    about = "A blazing-fast alternative to Google's repo tool"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a rupo workspace from a manifest URL
    Init {
        /// Manifest URL
        #[arg(short, long)]
        url: String,

        /// Branch or revision to use
        #[arg(short, long)]
        branch: Option<String>,

        /// Manifest filename within the repository
        #[arg(short, long, default_value = "default.xml")]
        manifest: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init {
            url,
            branch,
            manifest,
        } => {
            let work_dir = std::env::current_dir()?;
            rupo::cli::init::run(&url, branch.as_deref(), &manifest, &work_dir).await?;
        }
    }

    Ok(())
}
