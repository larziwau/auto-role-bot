use super::prelude::*;

/// Sync your discord roles with your GD account on Globed
#[poise::command(slash_command, guild_only = true)]
pub async fn sync(ctx: Context<'_>) -> Result<(), CommandError> {
    let state = ctx.data();
    let member = ctx.author_member().await.unwrap();

    ctx.defer_ephemeral().await?;

    match state.sync_roles(&member).await {
        Ok(()) => {
            ctx.reply("✅ Successfully synced roles! If you were already online on Globed, please reconnect to the server to see the changes.").await?;
        }

        Err(RoleSyncError::NotLinked) => {
            ctx.reply(":x: Not currently linked to any account. Use `/link` to link a GD account.")
                .await?;
        }

        Err(e) => {
            ctx.reply(":x: Failed to sync your roles due to an internal error.")
                .await?;

            bail!("Failed to sync roles ({}): {e}", ctx.author().name);
        }
    };

    Ok(())
}
