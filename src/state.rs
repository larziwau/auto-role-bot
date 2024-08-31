use std::{env, fmt::Display, num::NonZeroI32};

use crate::{db::*, serenity, Context};
use log::{debug, error, warn};
use parking_lot::RwLock as SyncRwLock;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serenity::all::{GuildId, Member, RoleId, UserId};

pub struct BotState {
    pub http_client: reqwest::Client,
    pub base_url: String,
    pub server_password: String,
    pub database: sqlx::SqlitePool,
    pub guild_id: GuildId,

    pub watched_roles: SyncRwLock<Vec<RoleId>>,
}

#[derive(Serialize)]
pub struct RoleSyncRequest {
    pub account_id: i32,
    pub keep: Vec<String>,
    pub remove: Vec<String>,
}

#[derive(Serialize)]
pub struct RoleSyncRequestData {
    pub users: Vec<RoleSyncRequest>,
}

pub enum RoleSyncError {
    NotLinked,
    Database(sqlx::Error),
    ServerRequest(reqwest::Error),
    #[allow(unused)]
    InternalError(&'static str),
    ServerUpdate((StatusCode, String)),
}

impl From<sqlx::Error> for RoleSyncError {
    fn from(value: sqlx::Error) -> Self {
        match value {
            sqlx::Error::RowNotFound => Self::NotLinked,
            v => Self::Database(v),
        }
    }
}

impl Display for RoleSyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotLinked => f.write_str("User not linked"),
            Self::Database(e) => write!(f, "Database error: {e}"),
            Self::ServerRequest(e) => write!(f, "Error making a request to the server: {e}"),
            Self::InternalError(e) => write!(f, "Internal error: {e}"),
            Self::ServerUpdate((code, message)) => {
                write!(f, "Server returned error (code {code}): {message}")
            }
        }
    }
}

pub enum RoleRemoveError {
    Database(sqlx::Error),
    NotFound,
}

impl From<sqlx::Error> for RoleRemoveError {
    fn from(value: sqlx::Error) -> Self {
        match value {
            sqlx::Error::RowNotFound => Self::NotFound,
            e => Self::Database(e),
        }
    }
}

pub enum LinkError {
    AlreadyLinked,
    InvalidUsername,
    ServerRequest(reqwest::Error),
    ServerInternalError(StatusCode, String),
    UserNotFound,
    ServerMalformedResponse(serde_json::Error, String),
    Database(sqlx::Error),
    RoleSync(RoleSyncError, UserLookupResponse),
    LinkedToOther(String),
}

impl From<sqlx::Error> for LinkError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(value)
    }
}

#[derive(Deserialize)]
pub struct UserLookupResponse {
    pub account_id: i32,
    pub name: String,
}

impl BotState {
    pub async fn new(database: sqlx::SqlitePool) -> Self {
        let mut base_url =
            env::var("BOT_BASE_URL").expect("'BOT_BASE_URL' env variable not passed");
        if base_url.ends_with('/') {
            base_url.pop();
        }

        let server_password =
            env::var("BOT_SERVER_PASSWORD").expect("'BOT_SERVER_PASSWORD' env variable not passed");

        let guild_id = GuildId::new(
            env::var("BOT_SERVER_ID")
                .expect("Expected BOT_SERVER_ID in environment")
                .parse()
                .expect("BOT_SERVER_ID must be an integer"),
        );

        // fetch roles

        let ret = Self {
            http_client: reqwest::Client::builder()
                .user_agent(format!(
                    "globed-game-server/discord-bot-{}",
                    env!("CARGO_PKG_VERSION")
                ))
                .build()
                .expect("Failed to create the HTTP client"),
            base_url,
            server_password,
            database,
            guild_id,
            watched_roles: SyncRwLock::new(Vec::new()),
        };

        // get all roles from the database and push them to a vec
        let roles = ret
            .get_all_roles()
            .await
            .expect("Failed to fetch roles from the database");

        let mut watched = ret.watched_roles.write();
        for role in roles {
            watched.push(RoleId::new(role.discord_id as u64));
        }

        #[cfg(debug_assertions)]
        debug!("new watched roles: {:?}", *watched);

        drop(watched);

        ret
    }

    /* Methods for linking/unlinking users etc. */

    pub async fn is_linked(&self, user_id: UserId) -> Result<bool, sqlx::Error> {
        Ok(self.get_linked_gd_account(user_id).await?.is_some())
    }

    pub async fn get_linked_gd_account(
        &self,
        user_id: UserId,
    ) -> Result<Option<NonZeroI32>, sqlx::Error> {
        let user_id = user_id.get() as i64;

        let res = sqlx::query_as!(
            LinkedUser,
            "SELECT * FROM linked_users WHERE id = ?",
            user_id
        )
        .fetch_one(&self.database)
        .await;

        match res {
            Ok(user) => Ok(NonZeroI32::new(user.gd_account_id as i32)),
            Err(sqlx::Error::RowNotFound) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub async fn get_linked_discord_account(
        &self,
        account_id: i32,
    ) -> Result<Option<UserId>, sqlx::Error> {
        let account_id = account_id as i64;

        let res = sqlx::query_as!(
            LinkedUser,
            "SELECT * FROM linked_users WHERE gd_account_id = ?",
            account_id
        )
        .fetch_one(&self.database)
        .await;

        match res {
            Ok(user) => Ok(Some(UserId::new(user.id as u64))),
            Err(sqlx::Error::RowNotFound) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub async fn link_user(
        &self,
        ctx: &Context<'_>,
        member: &Member,
        gd_username: &str,
    ) -> Result<UserLookupResponse, LinkError> {
        if !gd_username.is_ascii() || gd_username.len() > 16 {
            return Err(LinkError::InvalidUsername);
        }

        let response = match self
            .http_client
            .get(format!(
                "{}/gsp/lookup?username={}",
                self.base_url, gd_username
            ))
            .header("Authorization", &self.server_password)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                return Err(LinkError::ServerRequest(e));
            }
        };

        let status = response.status();
        if !status.is_success() {
            if status == StatusCode::NOT_FOUND {
                return Err(LinkError::UserNotFound);
            }

            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "<no message>".to_owned());

            return Err(LinkError::ServerInternalError(status, message));
        }

        let json = response.text().await.unwrap_or_default();
        let response: UserLookupResponse = match serde_json::from_str(&json) {
            Ok(x) => x,
            Err(err) => {
                return Err(LinkError::ServerMalformedResponse(err, json));
            }
        };

        let user_id_int = member.user.id.get() as i64;

        // insert into the db
        match sqlx::query!(
            "INSERT INTO linked_users (id, gd_account_id) VALUES (?, ?)",
            user_id_int,
            response.account_id
        )
        .execute(&self.database)
        .await
        {
            Ok(_) => {}
            Err(sqlx::Error::Database(err)) => {
                // this is pretty bad but eh
                if !err.message().contains("UNIQUE constraint failed") {
                    return Err(LinkError::Database(sqlx::Error::Database(err)));
                }

                // check if the someone else's discord is alreday linked to this gd account
                let linked_disc = self.get_linked_discord_account(response.account_id).await?;

                // if linked to someone else than us, tell the user
                if linked_disc.as_ref().is_some_and(|id| *id != member.user.id) {
                    let linked_id = linked_disc.unwrap();

                    // try to fetch the member and display their username, else fall back to their user id
                    let mut ident = String::new();

                    // god i fucking hate async rust
                    {
                        if let Some(cached) = ctx.cache().user(linked_id) {
                            ident.push('@');
                            ident.push_str(&cached.name);
                        }
                    }

                    if ident.is_empty() {
                        if let Ok(user) = ctx.http().get_user(linked_id).await {
                            ident.push('@');
                            ident.push_str(&user.name);
                        } else {
                            ident = linked_id.to_string();
                        }
                    };

                    return Err(LinkError::LinkedToOther(ident));
                } else {
                    // otherwise most likely we are already linked
                    return Err(LinkError::AlreadyLinked);
                }
            }
            Err(err) => {
                return Err(LinkError::Database(err));
            }
        }

        // sync roles
        match self.sync_roles(member).await {
            Ok(()) => Ok(response),
            Err(e) => Err(LinkError::RoleSync(e, response)),
        }
    }

    pub async fn unlink_user(&self, user_id: UserId) -> Result<(), RoleSyncError> {
        let user_id = user_id.get() as i64;

        // check if the user is linked
        let linked_user = sqlx::query_as!(
            LinkedUser,
            "SELECT * FROM linked_users WHERE id = ?",
            user_id
        )
        .fetch_one(&self.database)
        .await?;

        // fetch roles from the database
        let db_roles = sqlx::query_as!(Role, "SELECT * FROM roles")
            .fetch_all(&self.database)
            .await?;

        // remove user from the database
        sqlx::query!("DELETE FROM linked_users WHERE id = ?", user_id)
            .execute(&self.database)
            .await?;

        // sync roles with the server

        let mut removed = Vec::new();

        for role in db_roles {
            removed.push(role.id);
        }

        let req = RoleSyncRequest {
            account_id: linked_user.gd_account_id as i32,
            keep: Vec::new(),
            remove: removed,
        };

        self.send_sync_roles_req(&RoleSyncRequestData { users: vec![req] })
            .await
    }

    pub async fn get_all_linked_users(&self) -> Result<Vec<LinkedUser>, sqlx::Error> {
        sqlx::query_as!(LinkedUser, "SELECT * FROM linked_users")
            .fetch_all(&self.database)
            .await
    }

    /* Methods for adding/removing/getting linked roles */

    pub async fn add_role(&self, role_id: i64, globed_role_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "INSERT INTO roles (id, discord_id) VALUES (?, ?)",
            globed_role_id,
            role_id
        )
        .execute(&self.database)
        .await?;

        let id = RoleId::new(role_id as u64);

        let mut watched = self.watched_roles.write();
        if !watched.contains(&id) {
            watched.push(id);
        }

        watched.sort();

        #[cfg(debug_assertions)]
        debug!("new watched roles: {:?}", *watched);

        Ok(())
    }

    pub async fn remove_role(&self, role_id: i64) -> Result<(), RoleRemoveError> {
        let affected = sqlx::query!("DELETE FROM roles WHERE discord_id = ?", role_id)
            .execute(&self.database)
            .await?
            .rows_affected();

        if affected == 0 {
            return Err(RoleRemoveError::NotFound);
        }

        let id = RoleId::new(role_id as u64);

        let mut watched = self.watched_roles.write();
        if let Some(pos) = watched.iter().position(|x| *x == id) {
            watched.remove(pos);
        }

        #[cfg(debug_assertions)]
        debug!("new watched roles: {:?}", *watched);

        Ok(())
    }

    pub async fn remove_role_by_globed_id(&self, role: &str) -> Result<(), RoleRemoveError> {
        let deleted = sqlx::query_as!(Role, "DELETE FROM roles WHERE id = ? RETURNING *", role)
            .fetch_one(&self.database)
            .await?;

        let id = RoleId::new(deleted.discord_id as u64);

        let mut watched = self.watched_roles.write();
        if let Some(pos) = watched.iter().position(|x| *x == id) {
            watched.remove(pos);
        }

        #[cfg(debug_assertions)]
        debug!("new watched roles: {:?}", *watched);

        Ok(())
    }

    pub async fn get_all_roles(&self) -> Result<Vec<Role>, sqlx::Error> {
        sqlx::query_as!(Role, "SELECT * FROM roles")
            .fetch_all(&self.database)
            .await
    }

    /* Methods for syncing */

    pub async fn sync_roles(&self, user: &Member) -> Result<(), RoleSyncError> {
        let req = self.make_role_sync_request(user).await?;

        self.send_sync_roles_req(&RoleSyncRequestData { users: vec![req] })
            .await
    }

    pub async fn sync_all_members(&self, http: &serenity::Http) -> Result<usize, RoleSyncError> {
        // get all linked users
        let linked_users = self
            .get_all_linked_users()
            .await
            .expect("Failed to read the database");

        // get all linked roles
        let linked_roles = self
            .get_all_roles()
            .await
            .expect("Failed to read the database");

        let mut sync_data = RoleSyncRequestData {
            users: Vec::with_capacity(linked_users.len()),
        };

        // for fastest lookup, put all ids of linked users into a vec and sort it, so binary search can be applied later
        let mut linked_ids: Vec<u64> = linked_users.iter().map(|x| x.id as u64).collect();
        linked_ids.sort();

        // Perform quite a massive scan

        let mut after = None;

        loop {
            let members = match http.get_guild_members(self.guild_id, None, after).await {
                Ok(x) => x,
                Err(err) => {
                    warn!("Failed to fetch guild members: {err}");
                    break;
                }
            };

            if members.is_empty() {
                break;
            }

            after = Some(members.last().unwrap().user.id.get());

            // iterate over this member chunk, if any of them are linked, add them to sync list
            for member in members {
                let member_id = member.user.id.get();
                if linked_ids.binary_search(&member_id).is_ok() {
                    let req = self.make_role_sync_request_with(
                        &member,
                        linked_users
                            .iter()
                            .find(|x| x.id == member_id as i64)
                            .unwrap(), // unwrap should be safe
                        &linked_roles,
                    );

                    sync_data.users.push(req);
                }
            }
        }

        // send a mass sync request!
        self.send_sync_roles_req(&sync_data)
            .await
            .map(|()| sync_data.users.len())
    }

    pub async fn make_role_sync_request(
        &self,
        user: &Member,
    ) -> Result<RoleSyncRequest, RoleSyncError> {
        let user_id = user.user.id.get() as i64;

        // check if the user is linked
        let linked_user = sqlx::query_as!(
            LinkedUser,
            "SELECT * FROM linked_users WHERE id = ?",
            user_id
        )
        .fetch_one(&self.database)
        .await?;

        // fetch roles from the database
        let db_roles = self.get_all_roles().await?;

        Ok(self.make_role_sync_request_with(user, &linked_user, &db_roles))
    }

    pub fn make_role_sync_request_with(
        &self,
        user: &Member,
        linked_user: &LinkedUser,
        all_roles: &[Role],
    ) -> RoleSyncRequest {
        // depending on which roles the user has, make a vec of roles that should be kept, and roles that should be removed
        let mut kept = Vec::new();
        let mut removed = Vec::new();

        for role in all_roles {
            // check if user has that role on discord
            if user
                .roles
                .iter()
                .any(|id| id.get() as i64 == role.discord_id)
            {
                // add to list of roles to be kept
                kept.push(role.id.clone());
            } else {
                // add to list of roles to be removed
                removed.push(role.id.clone());
            }
        }

        #[cfg(debug_assertions)]
        debug!(
            "for {}, keep: {kept:?}, remove: {removed:?}",
            user.display_name()
        );

        /* make a request to update the roles on the central server */
        RoleSyncRequest {
            account_id: linked_user.gd_account_id as i32,
            keep: kept,
            remove: removed,
        }
    }

    // internal function for making server web request to sync roles
    pub async fn send_sync_roles_req(
        &self,
        data: &RoleSyncRequestData,
    ) -> Result<(), RoleSyncError> {
        let body: String = match serde_json::to_string(data) {
            Ok(x) => x,
            Err(err) => {
                error!("This should never fail: {err}");

                #[cfg(debug_assertions)]
                unreachable!();
                #[cfg(not(debug_assertions))]
                return Err(RoleSyncError::InternalError(
                    "internal error in serializing data",
                ));
            }
        };

        let response = match self
            .http_client
            .post(format!("{}/gsp/sync_roles", self.base_url))
            .header("Authorization", &self.server_password)
            .body(body)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                return Err(RoleSyncError::ServerRequest(e));
            }
        };

        let status = response.status();
        if !status.is_success() {
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "<no message>".to_owned());

            warn!(
                "Role update failed: code {}, message: {}",
                status.as_u16(),
                message
            );

            return Err(RoleSyncError::ServerUpdate((status, message)));
        }

        // success!
        Ok(())
    }
}
