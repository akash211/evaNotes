mod embedded {
    use refinery::embed_migrations;
    embed_migrations!("migrations");
}

pub fn run_migrations(conn: &mut rusqlite::Connection) -> Result<(), refinery::Error> {
    embedded::migrations::runner().run(conn)?;
    Ok(())
}
