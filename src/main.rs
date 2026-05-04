use clap::{Parser, Subcommand};
use rupo::sync::parallel::SyncOptions;

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

        /// Restrict manifest projects to specified group(s) [default|all|G1,G2,-G3]
        #[arg(short, long)]
        groups: Option<String>,
    },

    /// Sync all projects in the workspace
    Sync {
        /// Number of parallel jobs (default: number of CPU cores)
        #[arg(short, long)]
        jobs: Option<usize>,

        /// Only sync the current branch
        #[arg(short, long)]
        current_branch: bool,

        /// Shallow clone depth
        #[arg(long)]
        depth: Option<u32>,

        /// Override group filter for this sync only [default|all|G1,G2,-G3]
        #[arg(short, long)]
        groups: Option<String>,
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
            groups,
        } => {
            let work_dir = std::env::current_dir()?;
            rupo::cli::init::run(&url, branch.as_deref(), &manifest, groups.as_deref(), &work_dir)
                .await?;
        }
        Commands::Sync {
            jobs,
            current_branch,
            depth,
            groups,
        } => {
            let work_dir = std::env::current_dir()?;
            let opts = SyncOptions {
                jobs: jobs.unwrap_or_else(|| {
                    std::thread::available_parallelism()
                        .map(|n| n.get())
                        .unwrap_or(4)
                }),
                current_branch,
                depth,
            };
            rupo::cli::sync::run(&work_dir, opts, groups.as_deref()).await?;
        }
    }

    Ok(())
}
