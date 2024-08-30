use super::prelude::*;

#[poise::command(slash_command, guild_only = true)]
pub async fn unlink(ctx: Context<'_>) -> Result<(), CommandError> {
    let state = ctx.data();
    let member = ctx.author_member().await.unwrap();

    match state.handle_unlink(member.user.id).await {
        Ok(()) => {
            ctx.reply("Successfully unlinked the account!").await?;
        }

        Err(RoleSyncError::NotLinked) => {
            ctx.reply(":x: Not currently linked to any account.")
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
