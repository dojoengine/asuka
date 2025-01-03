use clap::{command, Parser};
use rig::providers::{self, anthropic, openai};
use sqlite_vec::sqlite3_vec_init;
use tokio_rusqlite::ffi::sqlite3_auto_extension;
use tokio_rusqlite::Connection;

use asuka_core::attention::{Attention, AttentionConfig};
use asuka_core::character;
use asuka_core::init_logging;
use asuka_core::knowledge::KnowledgeBase;
use asuka_core::loaders::{MultiLoader, MultiLoaderConfig};
use asuka_core::{agent::Agent, clients::discord::DiscordClient};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to character profile TOML file
    #[arg(long, default_value = "examples/src/characters/shinobi.toml")]
    character: String,

    /// Path to database
    #[arg(long, default_value = ":memory:")]
    db_path: String,

    /// Discord API token (can also be set via DISCORD_API_TOKEN env var)
    #[arg(long, env)]
    discord_api_token: String,

    /// XAI API token (can also be set via XAI_API_KEY env var)
    #[arg(long, env = "XAI_API_KEY")]
    xai_api_key: String,

    /// OpenAI API token (can also be set via OPENAI_API_KEY env var)
    #[arg(long, env = "OPENAI_API_KEY")]
    openai_api_key: String,

    /// Anthropic API token (can also be set via ANTHROPIC_API_KEY env var)
    #[arg(long, env = "ANTHROPIC_API_KEY")]
    anthropic_api_key: String,

    /// List of sources in format type:url (e.g. github:https://github.com/org/repo site:https://example.com)
    #[arg(
        long,
        value_delimiter = ' ',
        default_value = "github:https://github.com/cartridge-gg/docs site:https://contraptions.venkateshrao.com/p/towards-a-metaphysics-of-worlds"
    )]
    sources: Vec<String>,

    /// Local path to store downloaded content
    #[arg(long, default_value = ".sources")]
    sources_path: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    dotenv::dotenv().ok();

    let args = Args::parse();

    let character_content =
        std::fs::read_to_string(&args.character).expect("Failed to read character file");
    let character: character::Character =
        toml::from_str(&character_content).expect("Failed to parse character TOML");

    // Initialize clients
    let oai = providers::openai::Client::new(&args.openai_api_key);
    let anthropic = anthropic::ClientBuilder::new(&args.anthropic_api_key).build();

    let embedding_model = oai.embedding_model(openai::TEXT_EMBEDDING_3_SMALL);
    let completion_model = anthropic.completion_model(anthropic::CLAUDE_3_5_SONNET);
    let small_completion_model = anthropic.completion_model(anthropic::CLAUDE_3_HAIKU);

    // Initialize the `sqlite-vec`extension
    // See: https://alexgarcia.xyz/sqlite-vec/rust.html
    unsafe {
        sqlite3_auto_extension(Some(std::mem::transmute(sqlite3_vec_init as *const ())));
    }

    let conn = Connection::open(args.db_path).await?;
    let mut knowledge = KnowledgeBase::new(conn.clone(), embedding_model).await?;

    let loader = MultiLoader::new(
        MultiLoaderConfig {
            sources_path: args.sources_path,
        },
        completion_model.clone(),
    );

    knowledge
        .add_documents(loader.load_sources(args.sources).await?)
        .await?;

    let agent = Agent::new(character, completion_model, knowledge);

    let config = AttentionConfig {
        bot_names: vec![agent.character.name.clone()],
        ..Default::default()
    };
    let attention = Attention::new(config, small_completion_model);

    let discord = DiscordClient::new(agent, attention);
    discord.start(&args.discord_api_token).await?;

    Ok(())
}
