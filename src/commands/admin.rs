use crate::state::LinkError;

use super::prelude::*;

#[poise::command(slash_command, subcommands("link", "unlink", "sync", "syncall"))]
pub async fn admin(_ctx: Context<'_>) -> Result<(), CommandError> {
    // unreachable
    Ok(())
}

/// Link another user to their GD account
#[poise::command(slash_command)]
pub async fn link(
    ctx: Context<'_>,
    #[description = "User to link"] member: serenity::Member,
    #[description = "GD username"] name: String,
) -> Result<(), CommandError> {
    let state = ctx.data();

    if !has_manage_roles_perm(&ctx).await {
        ctx.reply(":x: No permission").await?;
        return Ok(());
    }

    match state.link_user(&ctx, &member, &name).await {
        Ok(user) => {
            ctx.reply(format!(
                "✅ Linked {} to GD account {} ({})!",
                member.user.name, user.name, user.account_id
            ))
            .await?;

            Ok(())
        }

        Err(LinkError::AlreadyLinked) => {
            ctx.reply(":x: This person is already linked. Use the `/unlink` command to unlink their account.")
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
            ctx.reply(":x: Failed to find the user by the given name. Make sure they are currently online on Globed and try again.").await?;
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
                "Linked the user to GD account @{} ({}) successfully, but role syncing failed. Try to execute the `/sync` command manually for them.",
                user.name,
                user.account_id
            )).await?;

            Ok(())
        }

        Err(LinkError::LinkedToOther(ident)) => {
            ctx.reply(format!(
                ":x: This Geometry Dash account is already linked to another Discord account ({}).",
                ident
            ))
            .await?;

            Ok(())
        }
    }
}

/// Unlink another user from their GD account
#[poise::command(slash_command)]
pub async fn unlink(
    ctx: Context<'_>,
    #[description = "User to unlink"] user: serenity::User,
) -> Result<(), CommandError> {
    let state = ctx.data();

    if !has_manage_roles_perm(&ctx).await {
        ctx.reply(":x: No permission").await?;
        return Ok(());
    }

    match state.unlink_user(user.id).await {
        Ok(()) => {
            ctx.reply("Successfully unlinked the user's account!")
                .await?;
        }

        Err(RoleSyncError::NotLinked) => {
            ctx.reply(":x: User is not linked to a GD account.").await?;
        }

        Err(e) => {
            ctx.reply(format!(":x: Error while unlinking user: {e}"))
                .await?;
        }
    }

    Ok(())
}

/// Sync another user's roles to their GD account on Globed
#[poise::command(slash_command)]
pub async fn sync(
    ctx: Context<'_>,
    #[description = "User to sync"] user: serenity::Member,
) -> Result<(), CommandError> {
    let state = ctx.data();

    if !has_manage_roles_perm(&ctx).await {
        ctx.reply(":x: No permission").await?;
        return Ok(());
    }

    match state.sync_roles(&user).await {
        Ok(()) => {
            ctx.reply(format!("✅ Successfully synced @{}'s roles! If they were already online on Globed, they might need to reconnect to the server to see the changes.", user.user.name)).await?;
        }

        Err(RoleSyncError::NotLinked) => {
            ctx.reply(":x: User is not linked to a GD account.").await?;
        }

        Err(e) => {
            ctx.reply(format!(":x: Error while syncing roles: {e}"))
                .await?;

            bail!("Error syncing user: {e}");
        }
    }

    Ok(())
}

/// Sync roles of all linked users on this server
#[poise::command(slash_command)]
pub async fn syncall(ctx: Context<'_>) -> Result<(), CommandError> {
    let state = ctx.data();

    if !has_manage_roles_perm(&ctx).await {
        ctx.reply(":x: No permission").await?;
        return Ok(());
    }

    match state.sync_all_members(ctx.http()).await {
        Ok(count) => {
            ctx.reply(format!("✅ Successfully synced roles of {count} people!"))
                .await?;
        }

        Err(e) => {
            ctx.reply(format!(":x: Error while syncing members: {e}"))
                .await?;

            bail!("Error syncing all members: {e}");
        }
    }

    Ok(())
}
