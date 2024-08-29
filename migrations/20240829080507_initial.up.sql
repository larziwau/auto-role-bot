-- Add migration script here
CREATE TABLE roles (
    id TEXT NOT NULL PRIMARY KEY,
    discord_id INTEGER NOT NULL
);

CREATE TABLE linked_users (
    id INTEGER NOT NULL PRIMARY KEY, -- discord id
    gd_account_id INTEGER UNIQUE NOT NULL
);
