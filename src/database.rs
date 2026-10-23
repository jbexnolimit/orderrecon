use mongodb::{Client, Database, bson::doc};

pub async fn connect(uri: &str, database_name: &str) -> Result<Database, mongodb::error::Error> {
    let client = Client::with_uri_str(uri).await?;

    let database = client.database(database_name);

    database.run_command(doc! { "ping": 1 }).await?;

    Ok(database)
}
