use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

use diesel::prelude::*;
use serde::Serialize;

use crate::schema::{page_revisions, pages};

pub const LANDING_SLUG: &str = "landing/landing";
pub type Error = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug)]
pub struct EditConflict;

impl std::fmt::Display for EditConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Page changed since it was opened; reopen it before saving. Your edit has not been saved.")
    }
}

impl std::error::Error for EditConflict {}

#[derive(Queryable, Identifiable, Serialize, Debug, Clone)]
#[diesel(table_name = pages)]
pub struct Page {
    pub id: i32,
    pub slug: String,
    pub content: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub creator: String,
    pub hidden: bool,
}

#[derive(Queryable, Identifiable, Serialize, Debug)]
#[diesel(table_name = page_revisions)]
pub struct Revision {
    pub id: i32,
    pub page_id: i32,
    pub content: String,
    pub hidden: bool,
    pub editor: String,
    pub edited_at: i64,
}

impl Page {
    pub fn title(&self) -> String {
        self.content
            .lines()
            .find_map(|line| {
                let line = line.trim();
                line.strip_prefix('#')
                    .map(|s| s.trim_start_matches('#').trim().to_owned())
            })
            .unwrap_or_else(|| {
                self.slug
                    .rsplit('/')
                    .next()
                    .unwrap_or(&self.slug)
                    .replace('-', " ")
            })
    }
}

pub fn normalize_slug(input: &str) -> Result<String, Error> {
    let input = input.trim().trim_end_matches('/');
    let input = input.strip_suffix(".md").unwrap_or(input);
    let parts: Vec<_> = input
        .split('/')
        .map(|s| s.trim().replace(' ', "-"))
        .collect();
    if parts.iter().any(|s| {
        s.is_empty()
            || s == "."
            || s == ".."
            || !s
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
    }) {
        return Err(
            "Invalid page slug: use names separated by '/', without empty or parent segments"
                .into(),
        );
    }
    Ok(parts.join("/"))
}

pub fn list(conn: &mut SqliteConnection, include_hidden: bool) -> QueryResult<Vec<Page>> {
    let mut query = pages::table.into_boxed();
    if !include_hidden {
        query = query.filter(pages::hidden.eq(false));
    }
    query
        .order((pages::updated_at.desc(), pages::id.desc()))
        .load(conn)
}

pub fn by_slug(conn: &mut SqliteConnection, slug: &str) -> QueryResult<Option<Page>> {
    pages::table
        .filter(pages::slug.eq(slug))
        .first(conn)
        .optional()
}

pub fn by_id(conn: &mut SqliteConnection, id: i32) -> QueryResult<Page> {
    pages::table.find(id).first(conn)
}

pub fn for_edit(conn: &mut SqliteConnection, id: i32) -> QueryResult<(Page, i32)> {
    conn.transaction(|conn| Ok((by_id(conn, id)?, latest_revision(conn, id)?)))
}

pub fn with_history(
    conn: &mut SqliteConnection,
    slug: &str,
) -> QueryResult<Option<(Page, Vec<Revision>)>> {
    conn.transaction(|conn| {
        by_slug(conn, slug)?
            .map(|page| {
                let revisions = history(conn, page.id)?;
                Ok((page, revisions))
            })
            .transpose()
    })
}

pub fn history(conn: &mut SqliteConnection, page_id: i32) -> QueryResult<Vec<Revision>> {
    page_revisions::table
        .filter(page_revisions::page_id.eq(page_id))
        .order(page_revisions::id.desc())
        .load(conn)
}

pub fn latest_revision(conn: &mut SqliteConnection, page_id: i32) -> QueryResult<i32> {
    page_revisions::table
        .filter(page_revisions::page_id.eq(page_id))
        .select(page_revisions::id)
        .order(page_revisions::id.desc())
        .first(conn)
}

pub fn editors(revisions: &[Revision]) -> Vec<String> {
    let mut editors = Vec::new();
    for revision in revisions.iter().rev() {
        if !editors.contains(&revision.editor) {
            editors.push(revision.editor.clone());
        }
    }
    editors
}

fn record(conn: &mut SqliteConnection, page: &Page, editor: &str) -> QueryResult<()> {
    diesel::insert_into(page_revisions::table)
        .values((
            page_revisions::page_id.eq(page.id),
            page_revisions::content.eq(&page.content),
            page_revisions::hidden.eq(page.hidden),
            page_revisions::editor.eq(editor),
            page_revisions::edited_at.eq(page.updated_at),
        ))
        .execute(conn)?;
    Ok(())
}

pub fn create(
    conn: &mut SqliteConnection,
    slug: &str,
    content: &str,
    creator: &str,
    hidden: bool,
) -> Result<Page, Error> {
    let now = chrono::Utc::now().timestamp();
    create_at(conn, slug, content, creator, hidden, now, now)
}

fn create_at(
    conn: &mut SqliteConnection,
    slug: &str,
    content: &str,
    creator: &str,
    hidden: bool,
    created_at: i64,
    updated_at: i64,
) -> Result<Page, Error> {
    let slug = normalize_slug(slug)?;
    let creator = creator.trim();
    if creator.is_empty() {
        return Err("A creator name is required".into());
    }
    conn.transaction::<_, Error, _>(|conn| {
        diesel::insert_into(pages::table)
            .values((
                pages::slug.eq(&slug),
                pages::content.eq(content),
                pages::creator.eq(creator),
                pages::hidden.eq(hidden),
                pages::created_at.eq(created_at),
                pages::updated_at.eq(updated_at),
            ))
            .execute(conn)?;
        let page = by_slug(conn, &slug)?.ok_or("Created page not found")?;
        record(conn, &page, creator)?;
        Ok(page)
    })
}

pub fn edit(
    conn: &mut SqliteConnection,
    id: i32,
    expected_revision: i32,
    content: &str,
    hidden: bool,
    editor: &str,
) -> Result<Page, Error> {
    let editor = editor.trim();
    if editor.is_empty() {
        return Err("An editor name is required".into());
    }
    conn.transaction::<_, Error, _>(|conn| {
        let mut page = by_id(conn, id)?;
        let latest = latest_revision(conn, id)?;
        if latest != expected_revision {
            return Err(EditConflict.into());
        }
        if page.content == content && page.hidden == hidden {
            return Ok(page);
        }
        page.content = content.to_owned();
        page.hidden = hidden;
        page.updated_at = chrono::Utc::now().timestamp();
        diesel::update(pages::table.find(id))
            .set((
                pages::content.eq(content),
                pages::hidden.eq(hidden),
                pages::updated_at.eq(page.updated_at),
            ))
            .execute(conn)?;
        record(conn, &page, editor)?;
        Ok(page)
    })
}

/// Explicit, transactional import. Existing SQL pages are never overwritten.
pub fn import_markdown(
    conn: &mut SqliteConnection,
    root: &Path,
    creator: &str,
) -> Result<usize, Error> {
    fn collect(root: &Path, dir: &Path, files: &mut Vec<std::path::PathBuf>) -> Result<(), Error> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                collect(root, &entry.path(), files)?;
            } else if entry.file_type()?.is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|s| s.eq_ignore_ascii_case("md"))
            {
                files.push(entry.path().strip_prefix(root)?.to_path_buf());
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    collect(root, root, &mut files)?;
    files.sort();
    conn.transaction::<_, Error, _>(|conn| {
        let mut imported = 0;
        for relative in &files {
            let slug = relative
                .with_extension("")
                .to_string_lossy()
                .replace('\\', "/");
            if by_slug(conn, &slug)?.is_some() {
                continue;
            }
            let path = root.join(relative);
            let metadata = path.metadata()?;
            let updated = metadata.modified()?.duration_since(UNIX_EPOCH)?.as_secs() as i64;
            let created = match metadata.created() {
                Ok(time) => time.duration_since(UNIX_EPOCH)?.as_secs() as i64,
                Err(error) if error.kind() == std::io::ErrorKind::Unsupported => updated,
                Err(error) => return Err(error.into()),
            };
            create_at(
                conn,
                &slug,
                &fs::read_to_string(path)?,
                creator,
                false,
                created.min(updated),
                updated,
            )?;
            imported += 1;
        }
        Ok(imported)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection() -> SqliteConnection {
        let mut conn = SqliteConnection::establish(":memory:").unwrap();
        crate::db::run_migrations(&mut conn).unwrap();
        conn
    }

    #[test]
    fn revisions_visibility_and_conflicts() {
        let mut conn = connection();
        let page = create(&mut conn, "draft.md", "# Draft", "Alice", true).unwrap();
        assert!(list(&mut conn, false).unwrap().is_empty());
        let initial = history(&mut conn, page.id).unwrap()[0].id;
        edit(&mut conn, page.id, initial, "# Draft", true, "Bob").unwrap();
        assert_eq!(history(&mut conn, page.id).unwrap().len(), 1);
        let published = edit(&mut conn, page.id, initial, "# Published", false, "Bob").unwrap();
        assert_eq!(published.created_at, page.created_at);
        assert_eq!(published.creator, "Alice");
        assert_eq!(list(&mut conn, false).unwrap().len(), 1);
        assert!(edit(&mut conn, page.id, initial, "stale", false, "Carol").is_err());
        let revisions = history(&mut conn, page.id).unwrap();
        assert_eq!(revisions[1].content, "# Draft");
        assert!(revisions[1].hidden);
        assert_eq!(editors(&revisions), vec!["Alice", "Bob"]);
        edit(&mut conn, page.id, revisions[0].id, "again", false, "Alice").unwrap();
        assert_eq!(
            editors(&history(&mut conn, page.id).unwrap()),
            vec!["Alice", "Bob"]
        );
    }

    #[test]
    fn rejects_invalid_input_and_duplicate_slugs() {
        let mut conn = connection();
        for slug in ["", "../bad", "a//b", "/a", "a\\b", "a?b"] {
            assert!(create(&mut conn, slug, "", "Alice", false).is_err());
        }
        assert!(create(&mut conn, "valid", "", " ", false).is_err());
        create(&mut conn, "folder/a-page.md", "one", "Alice", false).unwrap();
        assert!(create(&mut conn, "folder/a-page", "two", "Bob", false).is_err());
        assert_eq!(list(&mut conn, true).unwrap().len(), 1);
    }

    #[test]
    fn revision_failure_rolls_back_page_update() {
        use diesel::connection::SimpleConnection;
        let mut conn = connection();
        let page = create(&mut conn, "one", "original", "Alice", false).unwrap();
        let revision = latest_revision(&mut conn, page.id).unwrap();
        conn.batch_execute("CREATE TRIGGER reject_revision BEFORE INSERT ON page_revisions BEGIN SELECT RAISE(ABORT, 'test failure'); END;").unwrap();
        assert!(edit(&mut conn, page.id, revision, "changed", true, "Bob").is_err());
        let unchanged = by_id(&mut conn, page.id).unwrap();
        assert_eq!(unchanged.content, "original");
        assert!(!unchanged.hidden);
        assert_eq!(history(&mut conn, page.id).unwrap().len(), 1);
        assert!(create(&mut conn, "two", "new", "Alice", false).is_err());
        assert!(by_slug(&mut conn, "two").unwrap().is_none());
    }

    #[test]
    fn import_is_recursive_idempotent_and_atomic() {
        let mut conn = connection();
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("nested")).unwrap();
        fs::write(dir.path().join("first.md"), "# First").unwrap();
        fs::write(dir.path().join("nested").join("second.md"), "# Second").unwrap();
        assert_eq!(
            import_markdown(&mut conn, dir.path(), "Importer").unwrap(),
            2
        );
        let first = by_slug(&mut conn, "first").unwrap().unwrap();
        assert_eq!(first.creator, "Importer");
        assert_eq!(history(&mut conn, first.id).unwrap().len(), 1);
        let revision = latest_revision(&mut conn, first.id).unwrap();
        edit(&mut conn, first.id, revision, "SQL edit", false, "Bob").unwrap();
        assert_eq!(import_markdown(&mut conn, dir.path(), "Other").unwrap(), 0);
        assert_eq!(by_id(&mut conn, first.id).unwrap().content, "SQL edit");
        fs::write(dir.path().join("third.md"), "valid").unwrap();
        fs::write(dir.path().join("zbad.md"), [0xff]).unwrap();
        assert!(import_markdown(&mut conn, dir.path(), "Importer").is_err());
        assert!(by_slug(&mut conn, "third").unwrap().is_none());
    }
}
