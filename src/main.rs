use std::env;

pub use poise::serenity_prelude as serenity;

use serenity::prelude::*;

mod commands;
mod db;
mod logger;
mod state;

use commands::CommandError;
use logger::*;
use state::{BotState, RoleSyncError};

pub type Context<'a> = poise::Context<'a, BotState, CommandError>;

async fn on_error(error: poise::FrameworkError<'_, BotState, CommandError>) {
    match error {
        poise::FrameworkError::Setup { error, .. } => panic!("Failed to start bot: {:?}", error),
        poise::FrameworkError::Command { error, ctx, .. } => {
            warn!("Error in command `{}`: {:?}", ctx.command().name, error,);
        }
        error => {
            if let Err(e) = poise::builtins::on_error(error).await {
                warn!("Error while handling error: {}", e)
            }
        }
    }
}

async fn event_handler(
    _ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'_, BotState, CommandError>,
    state: &BotState,
) -> Result<(), CommandError> {
    match event {
        serenity::FullEvent::GuildMemberUpdate {
            old_if_available,
            new: Some(new),
            event: _event,
        } => {
            // check for the roles
            let mut should_sync = false;
            {
                let watched = state.watched_roles.read();

                if let Some(old_user) = old_if_available {
                    // iterate over all watched roles, see if anything changed
                    for role in &*watched {
                        if new.roles.contains(role) != old_user.roles.contains(role) {
                            should_sync = true;
                            break;
                        }
                    }
                } else {
                    should_sync = true;
                }
            }

            if should_sync {
                match state.sync_roles(new).await {
                    Ok(_) | Err(RoleSyncError::NotLinked) => {}
                    Err(err) => {
                        return Err(CommandError::other(format!(
                            "Failed to auto sync user roles: {err}"
                        )));
                    }
                }
            }
        }

        serenity::FullEvent::GuildMemberRemoval {
            guild_id: _guild_id,
            user,
            member_data_if_available: _member,
        } => {
            // if a user left, unlink and remove them
            match state.unlink_user(user.id).await {
                Ok(()) | Err(RoleSyncError::NotLinked) => {}
                Err(err) => {
                    return Err(CommandError::other(format!(
                        "Failed to unlink user that left the guild: {err}"
                    )));
                }
            }
        }

        _ => {}
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup logger

    let write_to_file = std::env::var("BOT_NO_FILE_LOG")
        .map(|p| p.parse::<i32>().unwrap())
        .unwrap_or(0)
        == 0;

    log::set_logger(Logger::instance("auto_role_bot", write_to_file)).unwrap();

    if let Some(log_level) = get_log_level("BOT_LOG_LEVEL") {
        log::set_max_level(log_level);
    } else {
        log::set_max_level(LogLevelFilter::Warn); // we have to print these logs somehow lol
        error!("invalid value for the log level environment varaible");
        warn!("hint: possible values are 'trace', 'debug', 'info', 'warn', 'error', and 'none'.");
        std::process::exit(1);
    }

    let token = env::var("BOT_TOKEN")
        .expect("No token set; please use the 'BOT_TOKEN' environment variable to pass it");

    // connect to db
    let db = sqlx::sqlite::SqlitePoolOptions::new().max_connections(5);

    let db = if let Ok(url) = env::var("DATABASE_URL") {
        db.connect(&url).await
    } else {
        db.connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename("db.sqlite")
                .create_if_missing(true),
        )
        .await
    };

    let db = db.expect("Couldn't connect to database, make sure the 'db.sqlite' file exists in the current directory or specify the 'DATABASE_URL' environment variable with the sqlite database URL.");

    // run migrations
    if let Err(e) = sqlx::migrate!().run(&db).await {
        error!("Feild to apply db migrations: {e:?}");
    }

    // start the discord bot
    let state = BotState::new(db).await;

    let options = poise::FrameworkOptions {
        commands: vec![
            commands::admin(),
            commands::link(),
            commands::unlink(),
            commands::role(),
            commands::sync(),
        ],
        on_error: |error| Box::pin(on_error(error)),
        command_check: Some(|ctx| {
            // only allow from a specific guild
            Box::pin(async move {
                if !ctx.guild_id().is_some_and(|g| g == ctx.data().guild_id) {
                    return Ok(false);
                }

                Ok(true)
            })
        }),

        event_handler: |ctx, event, framework, data| {
            Box::pin(event_handler(ctx, event, framework, data))
        },
        ..Default::default()
    };

    let framework = poise::Framework::builder()
        .setup(move |ctx, ready, framework| {
            Box::pin(async move {
                info!("Logged in as {}", ready.user.name);

                // register commands
                poise::builtins::register_in_guild(
                    ctx,
                    &framework.options().commands,
                    state.guild_id,
                )
                .await?;

                let skip_sync = env::var("BOT_SKIP_SYNC_ALL")
                    .ok()
                    .map(|x| x != "0")
                    .unwrap_or(false);

                if !skip_sync {
                    info!("Attempting to sync all members.. (this may take some time)");

                    match state.sync_all_members(&ctx.http).await {
                        Ok(count) => {
                            info!("Sync finished! Total {} users synced.", count);
                        }
                        Err(e) => {
                            warn!("Failed to sync roles of members: {e}");
                        }
                    }
                }

                Ok(state)
            })
        })
        .options(options)
        .build();

    let client = serenity::ClientBuilder::new(
        token,
        GatewayIntents::non_privileged() | GatewayIntents::GUILD_MEMBERS,
    )
    .framework(framework)
    .await;

    client.unwrap().start().await.unwrap();

    Ok(())
}

pub fn get_log_level(env_var: &str) -> Option<LogLevelFilter> {
    std::env::var(env_var).map_or_else(
        |_| {
            Some(if cfg!(debug_assertions) {
                LogLevelFilter::Trace
            } else {
                LogLevelFilter::Info
            })
        },
        |level| match &*level.to_lowercase() {
            "trace" => Some(LogLevelFilter::Trace),
            "debug" => Some(LogLevelFilter::Debug),
            "info" => Some(LogLevelFilter::Info),
            "warn" => Some(LogLevelFilter::Warn),
            "error" => Some(LogLevelFilter::Error),
            "off" => Some(LogLevelFilter::Off),
            _ => None,
        },
    )
}
