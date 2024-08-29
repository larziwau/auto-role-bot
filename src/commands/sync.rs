use super::prelude::*;

#[poise::command(slash_command, guild_only = true)]
pub async fn sync(
    ctx: Context<'_>,
    #[description = "User to sync roles for (mod only)"] user: Option<serenity::Member>,
) -> Result<(), CommandError> {
    let state = ctx.data();
    let member = ctx.author_member().await.unwrap();

    let for_self = user.is_none();

    // only mods can sync someone else's roles
    if !for_self && !has_manage_roles_perm(ctx).await {
        ctx.reply(":x: No permission to sync roles for another user.")
            .await?;

        return Ok(());
    }

    let synced_member = if let Some(m) = &user { m } else { &member };

    match state.sync_roles(synced_member).await {
        Ok(()) => {
            if for_self {
                ctx.reply("✅ Successfully synced roles! If you were already online on Globed, please reconnect to the server to see the changes.").await?;
            } else {
                ctx.reply(format!("✅ Successfully synced <@{}>'s roles! If they were already online on Globed, they might need to reconnect to the server to see the changes.", synced_member.user.id)).await?;
            }
        }

        Err(RoleSyncError::NotLinked) => {
            ctx.reply(":x: Not currently linked to any account. Use `/link` to link a GD account.")
                .await?;
        }

        Err(e) => {
            ctx.reply(":x: Failed to unlink your account due to an internal error.")
                .await?;

            bail!("Failed to unlink user ({}): {e}", ctx.author().name);
        }
    };

    Ok(())
}
