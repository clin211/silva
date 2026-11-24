use clap::{Parser, Subcommand, ValueEnum};
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
    /// 创建指定内置角色的账户；密码读取环境变量 `SILVA_USER_PASSWORD`，不接受命令行参数。
    CreateUser {
        #[arg(long, env = "SILVA_USER_EMAIL")]
        email: String,
        #[arg(long, env = "SILVA_USER_NAME")]
        display_name: String,
        /// 内置角色，可重复指定以分配多个角色，如 `--role author --role editor`。
        #[arg(long = "role", value_enum, required = true)]
        roles: Vec<RoleKey>,
    },
}

/// CLI 可分配的内置角色，取值与 `silva_domain::identity::ROLE_KEYS` 一致。
#[derive(Clone, Copy, ValueEnum)]
enum RoleKey {
    Author,
    Editor,
    Admin,
}

impl RoleKey {
    fn key(self) -> &'static str {
        match self {
            Self::Author => "author",
            Self::Editor => "editor",
            Self::Admin => "admin",
        }
    }
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
            let user = created(service.seed_admin(email, display_name, password).await)?;
            tracing::info!(user_id = %user.id, "administrator created");
        }
        Command::CreateUser {
            email,
            display_name,
            roles,
        } => {
            let password = std::env::var("SILVA_USER_PASSWORD")
                .map_err(|_| anyhow::anyhow!("SILVA_USER_PASSWORD is required"))?;
            let roles = roles.iter().map(|role| role.key().to_owned()).collect();
            let service = identity_service(pool.clone(), config.session_ttl).await?;
            let user = created(
                service
                    .provision_user(email, display_name, password, roles)
                    .await,
            )?;
            tracing::info!(user_id = %user.id, roles = ?user.roles, "user created");
        }
    }
    Ok(())
}

/// 邮箱已存在时给出明确提示，且保证未改动已有账户。
fn created<T>(result: silva_shared::error::Result<T>) -> anyhow::Result<T> {
    match result {
        Ok(user) => Ok(user),
        Err(silva_shared::error::AppError::Conflict(silva_shared::error::Conflict::EmailInUse)) => {
            anyhow::bail!("account already exists; no password or role was changed")
        }
        Err(error) => Err(error.into()),
    }
}
