use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "devpulse", version, about = "Unified Developer Environment TUI")]
pub struct Cli {
    #[arg(short, long)]
    pub config: Option<PathBuf>,
    #[arg(short, long)]
    pub filter: Option<String>,
    #[arg(short, long)]
    pub layout: Option<String>,
    #[arg(long)]
    pub no_docker: bool,
    #[arg(long)]
    pub refresh: Option<u64>,
}

fn main() {
    let _cli = Cli::parse();
    println!("devpulse v0.1.0");
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_default_cli_args() {
        let args = Cli::parse_from(["devpulse"]);
        assert!(args.config.is_none());
        assert!(args.filter.is_none());
        assert_eq!(args.layout, None);
        assert!(!args.no_docker);
        assert!(args.refresh.is_none());
    }

    #[test]
    fn test_cli_args_with_options() {
        let args = Cli::parse_from([
            "devpulse", "--config", "/tmp/config.toml",
            "--filter", "node", "--layout", "quad",
            "--no-docker", "--refresh", "5000",
        ]);
        assert_eq!(args.config.unwrap().to_str().unwrap(), "/tmp/config.toml");
        assert_eq!(args.filter.unwrap(), "node");
        assert_eq!(args.layout.unwrap(), "quad");
        assert!(args.no_docker);
        assert_eq!(args.refresh.unwrap(), 5000);
    }
}
