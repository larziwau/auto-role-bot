// use sqlx::prelude::FromRow;

#[derive(Clone, Debug)]
pub struct Role {
    pub id: String,
    pub discord_id: i64,
}

#[derive(Clone, Debug)]
pub struct LinkedUser {
    #[allow(unused)]
    pub id: i64,
    pub gd_account_id: i64,
}
