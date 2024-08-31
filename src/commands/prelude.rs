// Imports typically needed for most commands
#[allow(unused)]
pub use super::{bail, has_admin_perm, has_manage_roles_perm, reply_ephemeral, CommandError};

#[allow(unused)]
pub use crate::{
    logger::*,
    serenity,
    state::{BotState, RoleRemoveError, RoleSyncError, RoleSyncRequest, RoleSyncRequestData},
    Context,
};
