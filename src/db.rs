use anyhow::{Context, Result};
use diesel::prelude::*;
use diesel::Connection;
diesel_migrations::embed_migrations!("./migrations/");

pub fn establish_connection() -> Result<SqliteConnection> {
    let db_url = "rustygolem.sqlite";
    // let db_url = "lambdacoucou.sqlite";
    SqliteConnection::establish(&db_url).context(format!("cannot connect to db at {}", db_url))
}

pub fn run_migrations(connection: &SqliteConnection) -> Result<()> {
    embedded_migrations::run(connection)
        .context("Cannot run migration")
        .map_err(|e| e.into())
}
