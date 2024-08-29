use std::{borrow::Cow, fmt::Display};

pub mod prelude;

mod link;
mod role;
mod sync;
mod unlink;

pub use link::*;
pub use role::role;
pub use sync::*;
pub use unlink::*;

#[derive(Debug)]
pub enum CommandError {
    Other(String),
    Serenity(super::serenity::Error),
    PrivateMessages,
}

impl Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Other(s) => f.write_str(s),
            Self::Serenity(e) => write!(f, "Serenity error: {e}"),
            Self::PrivateMessages => {
                f.write_str("Attempted to invoke a command in private messages")
            }
        }
    }
}

impl From<super::serenity::Error> for CommandError {
    fn from(value: super::serenity::Error) -> Self {
        Self::Serenity(value)
    }
}

impl CommandError {
    pub fn other<T: Into<Cow<'static, str>>>(inner: T) -> Self {
        let inner = match inner.into() {
            Cow::Borrowed(borrowed) => borrowed.to_owned(),
            Cow::Owned(owned) => owned,
        };

        Self::Other(inner)
    }
}

#[macro_export]
macro_rules! bail {
    ($($data:tt)*) => {
        return Err(CommandError::other(format!($($data)*)));
    };

    ($data:literal) => {
        return Err(CommandError::other($data));
    };
}

pub use bail;

pub async fn has_admin_perm(ctx: crate::Context<'_>) -> bool {
    ctx.author_member()
        .await
        .unwrap()
        .permissions
        .is_some_and(|p| p.administrator())
}

pub async fn has_manage_roles_perm(ctx: crate::Context<'_>) -> bool {
    ctx.author_member()
        .await
        .unwrap()
        .permissions
        .is_some_and(|p| p.manage_roles())
}
