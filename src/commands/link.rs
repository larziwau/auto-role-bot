use crate::state::LinkError;

use super::prelude::*;

/// Link your Discord account to your GD account, to get roles on Globed
#[poise::command(slash_command, guild_only = true)]
pub async fn link(
    ctx: Context<'_>,
    #[description = "GD username"] username: String,
) -> Result<(), CommandError> {
    let state = ctx.data();

    let member = ctx.author_member().await.unwrap();

    ctx.defer_ephemeral().await?;

    match state.link_user(&ctx, &member, &username).await {
        Ok(user) => {
            ctx.reply(format!(
                "✅ Linked <@{}> to GD account {} ({})!",
                ctx.author().id,
                user.name,
                user.account_id
            ))
            .await?;

            Ok(())
        }

        Err(LinkError::AlreadyLinked) => {
            ctx.reply(":x: Already linked. Use the `/unlink` command to unlink your account.")
                .await?;

            Ok(())
        }

        Err(LinkError::InvalidUsername) => {
            ctx.reply(":x: Invalid username was provided.").await?;
            Ok(())
        }

        Err(LinkError::ServerRequest(err)) => {
            ctx.reply(":x: Failed to make a request to the server!")
                .await?;

            bail!("User lookup failed: {err}");
        }

        Err(LinkError::ServerInternalError(status, message)) => {
            ctx.reply(":x: Server returned an unexpected error!")
                .await?;

            bail!(
                "User lookup failed: code {}, message: {}",
                status.as_u16(),
                message
            );
        }

        Err(LinkError::UserNotFound) => {
            ctx.reply(":x: Failed to find the user by the given name. Make sure you are currently online on Globed and try again.").await?;
            Ok(())
        }

        Err(LinkError::ServerMalformedResponse(error, json)) => {
            ctx.reply(":x: Server returned unparsable data.").await?;
            bail!("User lookup failed: failed to parse response: {error:?}\nResponse was: {json}");
        }

        Err(LinkError::Database(err)) => {
            ctx.reply(":x: Unknown database error has occurred.")
                .await?;

            bail!("database connection error: {err}");
        }

        Err(LinkError::RoleSync(err, user)) => {
            warn!("Failed to sync roles: {err}");

            ctx.reply(format!(
                "Linked <@{}> to GD account {} ({}) successfully, but role syncing failed. Try to execute the `/sync` command manually, or contact staff for assistance.",
                ctx.author().id,
                user.name,
                user.account_id
            )).await?;

            Ok(())
        }

        Err(LinkError::LinkedToOther(ident)) => {
            ctx.reply(format!(":x: This Geometry Dash account is already linked to another Discord account ({}). If this is not you, please contact the moderator team.", ident))
            .await?;

            Ok(())
        }
    }
}
