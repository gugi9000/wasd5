use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;

use clap::{Parser, Subcommand};
use diesel::prelude::*;
use wasd5::{db, models::User, pages};

#[derive(Parser)]
#[command(author, version, about = "Manage SQL pages and revision history")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all pages, including hidden drafts, with IDs
    List,
    /// Open a page in $EDITOR (edit a temporary Markdown file, then save to SQL)
    Edit {
        id: i32,
        /// Attribution name; otherwise use WASD_PAGE_EDITOR or prompt
        #[arg(long)]
        editor: Option<String>,
    },
    /// Create a page from a Markdown file
    Create {
        slug: String,
        file: PathBuf,
        #[arg(long)]
        hidden: bool,
        #[arg(long)]
        editor: Option<String>,
    },
    /// Hide a page from everyone except admins
    Hide {
        id: i32,
        #[arg(long)]
        editor: Option<String>,
    },
    /// Publish a hidden page
    Publish {
        id: i32,
        #[arg(long)]
        editor: Option<String>,
    },
    /// Show full revision snapshots and editor attribution as JSON
    History { id: i32 },
    /// Import legacy Markdown without overwriting existing SQL pages
    Import {
        #[arg(default_value = "pages")]
        directory: PathBuf,
        /// Attribute imports to this existing user's username
        #[arg(long)]
        creator_id: i32,
    },
}

fn editor_name(override_name: Option<String>) -> Result<String, pages::Error> {
    if let Some(name) = override_name.or_else(|| env::var("WASD_PAGE_EDITOR").ok()) {
        if name.trim().is_empty() {
            return Err(
                "Editor name must not be empty; set WASD_PAGE_EDITOR or use --editor".into(),
            );
        }
        return Ok(name.trim().to_owned());
    }
    eprintln!("WASD_PAGE_EDITOR is not set. Set it to your attribution name to avoid this prompt:");
    eprintln!("  PowerShell: $env:WASD_PAGE_EDITOR = 'Your name'");
    eprintln!("  sh: export WASD_PAGE_EDITOR='Your name'");
    eprint!("Who is editing this page? ");
    io::stderr().flush()?;
    let mut name = String::new();
    io::stdin().read_line(&mut name)?;
    if name.trim().is_empty() {
        return Err("An editor name is required; set WASD_PAGE_EDITOR or pass --editor".into());
    }
    Ok(name.trim().to_owned())
}

fn edit_in_editor(pool: &db::DbPool, id: i32, name: &str) -> Result<(), pages::Error> {
    let command = env::var("EDITOR").map_err(|_| {
        "EDITOR is not set. Set $env:EDITOR = 'code --wait' in PowerShell, or export EDITOR='vi' in sh"
    })?;
    let args = shell_words::split(&command)?;
    let (program, args) = args.split_first().ok_or("EDITOR must not be empty")?;
    let (page, revision) = {
        let mut conn = pool.get()?;
        pages::for_edit(&mut conn, id)?
    };
    let mut file = tempfile::Builder::new()
        .prefix("wasd5-page-")
        .suffix(".md")
        .tempfile()?;
    file.write_all(page.content.as_bytes())?;
    file.flush()?;
    let file = file.into_temp_path();
    let result = (|| -> Result<(), pages::Error> {
        let status = Command::new(program).args(args).arg(&file).status()?;
        if !status.success() {
            return Err(format!("EDITOR exited with {status}; SQL page was not changed").into());
        }
        let content = std::fs::read_to_string(&file)?;
        let mut conn = pool.get()?;
        pages::edit(&mut conn, id, revision, &content, page.hidden, name)?;
        if content == page.content {
            println!("No changes; no revision added.");
        } else {
            println!("Saved page {id}.");
        }
        Ok(())
    })();
    if result.is_err() {
        let path = file.keep()?;
        eprintln!(
            "Your temporary Markdown file was kept at {}",
            path.display()
        );
    }
    result
}

fn main() -> Result<(), pages::Error> {
    let cli = Cli::parse();
    let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| "wasd5.db".into());
    let pool = db::establish_pool(&database_url);
    {
        let mut conn = pool.get()?;
        db::run_migrations(&mut conn)?;
    }
    let hide = matches!(cli.command, Commands::Hide { .. });
    match cli.command {
        Commands::List => {
            let mut conn = pool.get()?;
            println!("ID\tSTATE\tSLUG\tCREATOR\tCREATED (UNIX)\tLAST EDIT (UNIX)");
            for page in pages::list(&mut conn, true)? {
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}",
                    page.id,
                    if page.hidden { "draft" } else { "published" },
                    page.slug,
                    page.creator,
                    page.created_at,
                    page.updated_at
                );
            }
        }
        Commands::Edit { id, editor } => edit_in_editor(&pool, id, &editor_name(editor)?)?,
        Commands::Create {
            slug,
            file,
            hidden,
            editor,
        } => {
            let name = editor_name(editor)?;
            let content = std::fs::read_to_string(file)?;
            let mut conn = pool.get()?;
            let page = pages::create(&mut conn, &slug, &content, &name, hidden)?;
            println!("Created page {} ({})", page.id, page.slug);
        }
        Commands::Hide { id, editor } | Commands::Publish { id, editor } => {
            let hidden = hide;
            let name = editor_name(editor)?;
            let mut conn = pool.get()?;
            let (page, revision) = pages::for_edit(&mut conn, id)?;
            pages::edit(&mut conn, id, revision, &page.content, hidden, &name)?;
            println!(
                "Page {id} is {}.",
                if hidden { "hidden" } else { "published" }
            );
        }
        Commands::History { id } => {
            let mut conn = pool.get()?;
            pages::by_id(&mut conn, id)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&pages::history(&mut conn, id)?)?
            );
        }
        Commands::Import {
            directory,
            creator_id,
        } => {
            let mut conn = pool.get()?;
            let user: User = wasd5::schema::users::table
                .find(creator_id)
                .first(&mut conn)?;
            let count = pages::import_markdown(&mut conn, &directory, &user.username)?;
            println!(
                "Imported {count} pages; creator: {} (user {creator_id}). Existing SQL pages were left unchanged.",
                user.username
            );
        }
    }
    Ok(())
}
