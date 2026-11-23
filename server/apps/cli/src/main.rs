use clap::{Parser, Subcommand};
use silva_infrastructure::{identity_service, persistence};
use silva_shared::{config::Config, telemetry};

#[derive(Parser)]
#[command(about = "Silva backend operations")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Migrate,
    CheckSchema,
    SeedAdmin {
        #[arg(long, env = "SILVA_ADMIN_EMAIL")]
        email: String,
        #[arg(long, env = "SILVA_ADMIN_NAME", default_value = "Administrator")]
        display_name: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    silva_shared::config::load_dotenv()?;
    let args = Args::parse();
    let config = Config::load()?;
    let telemetry = telemetry::init("silva-cli")?;
    let result = run(config, args.command).await;
    telemetry.shutdown().await;
    result
}

async fn run(config: Config, command: Command) -> anyhow::Result<()> {
    let pool = persistence::connect(&config).await?;
    let result = execute(&pool, &config, command).await;
    pool.close().await;
    result
}

async fn execute(pool: &sqlx::PgPool, config: &Config, command: Command) -> anyhow::Result<()> {
    match command {
        Command::Migrate => {
            persistence::migrate(pool).await?;
            tracing::info!("migrations and built-in roles applied");
        }
        Command::CheckSchema => {
            persistence::check_schema(pool).await?;
            tracing::info!("schema comments and UUIDv7 invariants verified");
        }
        Command::SeedAdmin {
            email,
            display_name,
        } => {
            let password = std::env::var("SILVA_ADMIN_PASSWORD")
                .map_err(|_| anyhow::anyhow!("SILVA_ADMIN_PASSWORD is required"))?;
            let service = identity_service(pool.clone(), config.session_ttl).await?;
            match service.seed_admin(email, display_name, password).await {
                Ok(user) => tracing::info!(user_id = %user.id, "administrator created"),
                Err(silva_shared::error::AppError::Conflict(
                    silva_shared::error::Conflict::EmailInUse,
                )) => {
                    anyhow::bail!("account already exists; no password or role was changed")
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(())
}
