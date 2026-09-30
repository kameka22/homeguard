use std::path::PathBuf;

use clap::{Parser, Subcommand};
use homeguard_agent::config::AgentPaths;
use homeguard_agent::identity::AgentIdentity;
use homeguard_agent::run::RunArgs;
use homeguard_agent::{enroll, run, service};

#[derive(Parser)]
#[command(name = "homeguard-agent", version, about = "Agent de blocage de sites (fichier hosts)")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the agent in the foreground (what the installed service invokes).
    Run {
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long)]
        hosts_path: Option<PathBuf>,
        #[arg(long, default_value_t = 7878)]
        port: u16,
        #[arg(long)]
        no_mdns: bool,
    },
    /// Generate a local key, request a signed certificate from the app,
    /// and persist the resulting identity + initial policy.
    Enroll {
        /// The app's temporary enrollment server, e.g. https://192.168.1.20:7879/v1/enroll
        #[arg(long)]
        url: String,
        #[arg(long)]
        token: String,
        #[arg(long = "ca-sha256")]
        ca_sha256: String,
        #[arg(long)]
        data_dir: Option<PathBuf>,
    },
    /// Register the OS service so the agent starts on boot for every
    /// session (must already be enrolled).
    Install {
        #[arg(long)]
        data_dir: Option<PathBuf>,
    },
    /// Restore the hosts file, remove the OS service, and wipe local
    /// agent data.
    Uninstall {
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long)]
        hosts_path: Option<PathBuf>,
        #[arg(long)]
        etc_root: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Run {
            data_dir,
            hosts_path,
            port,
            no_mdns,
        } => {
            let args = RunArgs {
                data_dir,
                hosts_path,
                port,
                mdns_enabled: !no_mdns,
            };
            // On Windows, when this process was actually launched by the
            // Service Control Manager, this blocks for the service's whole
            // lifetime and drives it through `service.rs`'s dispatcher
            // instead of the plain foreground path below. It returns
            // `None` on every other platform, and on Windows too when an
            // admin just ran `homeguard-agent run` by hand.
            match service::try_run_as_service(args.clone()) {
                Some(result) => result.map_err(anyhow::Error::from),
                None => run::run(args, None).await,
            }
        }
        Command::Enroll {
            url,
            token,
            ca_sha256,
            data_dir,
        } => {
            let paths = data_dir.map(AgentPaths::new).unwrap_or_else(AgentPaths::default_for_os);
            paths.ensure_created()?;
            let enrolled = enroll::enroll(&paths, &url, &token, &ca_sha256).await?;
            println!("Enrôlé avec succès. agent_id = {}", enrolled.agent_id);
            Ok(())
        }
        Command::Install { data_dir } => {
            let paths = data_dir.map(AgentPaths::new).unwrap_or_else(AgentPaths::default_for_os);
            if AgentIdentity::load(&paths).is_err() {
                anyhow::bail!("l'agent n'est pas encore enrôlé — lancez d'abord `homeguard-agent enroll`");
            }
            let exe = std::env::current_exe()?;
            service::install(&exe, &paths.data_dir)?;
            println!("Service installé et démarré.");
            Ok(())
        }
        Command::Uninstall {
            data_dir,
            hosts_path,
            etc_root,
        } => {
            let paths = data_dir.map(AgentPaths::new).unwrap_or_else(AgentPaths::default_for_os);
            let hosts_path = hosts_path.unwrap_or_else(AgentPaths::system_hosts_path);
            let etc_root = etc_root.unwrap_or_else(|| PathBuf::from("/etc"));
            service::perform_uninstall(&paths, &hosts_path, &etc_root)?;
            println!("Agent désinstallé, hosts restauré, politiques navigateurs levées.");
            Ok(())
        }
    }
}
