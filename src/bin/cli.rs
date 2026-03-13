use std::env;

use clap::{Parser, Subcommand};
use bcrypt::hash;
use chrono::Utc;

use wasd5::db;
use wasd5::models;
use diesel::prelude::*;
use serde_json;

#[derive(Parser)]
#[command(author, version, about = "wasd5 admin CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new user
    CreateUser {
        /// username
        username: String,
        /// password
        password: String,
        /// role (admin/member)
        #[arg(short, long)]
        role: Option<String>,
    },
    /// List users
    ListUsers,
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    let cli = Cli::parse();
    let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| "wasd5.db".to_string());
    let pool = db::establish_pool(&database_url);

    // ensure migrations applied
    {
        let mut conn = pool.get()?;
        db::run_migrations(&mut conn)?;
    }

    match cli.command {
        Commands::CreateUser { username, password, role } => {
            use wasd5::schema::users::dsl::users;
            let role_val = role.unwrap_or_else(|| "member".to_string());
            let pw_hash = hash(&password, bcrypt::DEFAULT_COST)?;
            let new = models::NewUser {
                username: username.as_str(),
                password_hash: &pw_hash,
                role: &role_val,
                created_at: chrono::Utc::now().timestamp(),
            };
            let mut conn = pool.get()?;
            diesel::insert_into(users).values(&new).execute(&mut conn)?;
            println!("created user {}", username);
        }
        Commands::ListUsers => {
            use wasd5::schema::users::dsl::{users, created_at};
            let mut conn = pool.get()?;
            let results = users.order(created_at.desc()).load::<models::User>(&mut conn)?;
            println!("{}", serde_json::to_string_pretty(&results)?);
        }
    }

    Ok(())
}
