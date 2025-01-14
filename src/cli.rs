use clap::builder::TypedValueParser;
use clap::Parser;
use tracing_subscriber::filter::LevelFilter;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Cli {
    /// Log level
    #[arg(
        long,
        env = "LOG_LEVEL",
        default_value_t = LevelFilter::INFO,
        value_parser = clap::builder::PossibleValuesParser::new(["trace", "debug", "info", "warn", "error"])
            .map(|s| s.parse::<LevelFilter>().unwrap()),
    )]
    pub log_level: LevelFilter,

    /// ID of the claimant - must be unique
    #[clap(long, env = "CLAIMANT")]
    pub claimant: String,
}
