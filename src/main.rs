use std::path::PathBuf;
use std::{collections::HashMap, env};

use chrono::Utc;
use pulldown_cmark::{Options, Parser, html};
use rocket::async_trait;
use rocket::catch;
use rocket::catchers;
use rocket::fs::FileServer;
use rocket::http::Status;
use rocket::http::{Cookie, CookieJar};
use rocket::request::{self, FromRequest, Outcome, Request};
use rocket::response::Redirect;
use rocket::{State, get, routes};
use rocket_dyn_templates::tera;
use rocket_dyn_templates::{Template, context};
use serde::Serialize;
use uuid::Uuid;
use wasd5::pages as page_store;

mod admin;
mod calendar;
mod comics;
mod creatine;
mod db;
mod helpers;
mod models;
mod packages;
mod schema;
use admin::{
    admin_calendar_settings_get, admin_create_picture_folder, admin_edit_page_get,
    admin_edit_page_post, admin_files_get, admin_index, admin_landing_get, admin_landing_post,
    admin_login_get, admin_login_post, admin_logout, admin_pages_new_get, admin_pages_new_post,
    admin_pictures_get, admin_rename_move_picture, admin_update_calendar_allowed_ips,
    admin_upload_file, admin_upload_picture, admin_users, admin_users_create, admin_users_new,
    admin_users_update, create_user, list_users,
};
use calendar::{
    api_calendar_access, api_calendar_create, api_calendar_delete, api_calendar_get,
    api_calendar_update, calendar_index,
};
use creatine::{creatine_delete, creatine_index, creatine_log, creatine_reminder_toggle};
use helpers::format_modified;
use packages::{create_package, delete_package, list_packages, mark_received, update_tracking};

const STATIC_DIR: &str = "static";

#[derive(Serialize, Clone)]
pub(crate) struct PageListing {
    id: i32,
    slug: String,
    title: String,
    modified: u64,
    hidden: bool,
}

pub(crate) struct PageNavigation(Vec<PageListing>);

#[async_trait]
impl<'r> FromRequest<'r> for PageNavigation {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> request::Outcome<Self, Self::Error> {
        let Some(pool) = req.rocket().state::<db::DbPool>() else {
            eprintln!("Page database pool is unavailable");
            return Outcome::Error((Status::InternalServerError, ()));
        };
        let result = (|| -> Result<_, page_store::Error> {
            let mut conn = pool.get()?;
            Ok(page_store::list(&mut conn, is_admin_cookie(req.cookies()))?
                .into_iter()
                .map(|page| PageListing {
                    id: page.id,
                    title: page.title(),
                    slug: page.slug,
                    modified: page.updated_at.max(0) as u64,
                    hidden: page.hidden,
                })
                .collect())
        })();
        match result {
            Ok(pages) => Outcome::Success(PageNavigation(pages)),
            Err(error) => {
                eprintln!("Failed to load page navigation: {error}");
                Outcome::Error((Status::InternalServerError, ()))
            }
        }
    }
}

pub(crate) fn page_error(error: impl std::fmt::Display) -> Status {
    eprintln!("Page operation failed: {error}");
    Status::InternalServerError
}

#[derive(Serialize)]
struct PageItem {
    slug: String,
    title: String,
    modified: u64,
    modified_str: String,
    modified_rel: String,
}

#[derive(Serialize)]
struct LatestItem {
    slug: String,
    title: String,
    modified_rel: String,
}

#[derive(Serialize)]
struct PageMetadata {
    created_at: String,
    last_updated: String,
    creator: String,
    editors: Vec<String>,
    hidden: bool,
}

fn page_metadata(
    page: &page_store::Page,
    conn: &mut diesel::SqliteConnection,
) -> Result<PageMetadata, Status> {
    let revisions = page_store::history(conn, page.id).map_err(page_error)?;
    let (abs, rel) = format_modified(page.updated_at.max(0) as u64);
    Ok(PageMetadata {
        created_at: format_modified(page.created_at.max(0) as u64).0,
        last_updated: format!("{abs} ({rel})"),
        creator: page.creator.clone(),
        editors: page_store::editors(&revisions),
        hidden: page.hidden,
    })
}

fn markdown_to_html(md: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(md, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    html_output
}

fn slug_to_category(slug: &str) -> String {
    if let Some((category, _)) = slug.split_once('/') {
        if category.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", category.to_lowercase())
        }
    } else {
        "/".to_string()
    }
}

fn category_filter(
    value: &tera::Value,
    _args: &HashMap<String, tera::Value>,
) -> tera::Result<tera::Value> {
    let slug = value
        .as_str()
        .ok_or_else(|| tera::Error::msg("category filter expects a string"))?;
    tera::to_value(slug_to_category(slug)).map_err(|e| tera::Error::msg(e.to_string()))
}

#[test]
fn test_slug_to_path() {
    assert_eq!(slug_to_category("landing/landing"), "/landing");
    assert_eq!(slug_to_category("singleword"), "/");
    assert_eq!(slug_to_category("two-words"), "/");
    assert_eq!(
        slug_to_category("realy/deep/category/words"),
        "/realy/deep/category"
    );
    assert_eq!(slug_to_category(""), "/");
}

#[get("/")]
fn index(
    jar: &CookieJar,
    navigation: PageNavigation,
    pool: &State<db::DbPool>,
) -> Result<Template, Status> {
    let pages = navigation.0;
    let mut landing_md = String::from(
        "# Welcome\n\nWelcome to the simple Rocket + Tera site serving Markdown pages.",
    );
    let mut conn = pool.get().map_err(page_error)?;
    let mut metadata = None;
    if let Some(page) =
        page_store::by_slug(&mut conn, page_store::LANDING_SLUG).map_err(page_error)?
    {
        if !page.hidden || is_admin_cookie(jar) {
            metadata = Some(page_metadata(&page, &mut conn)?);
            landing_md = page.content;
        }
    }
    let landing_html = markdown_to_html(&landing_md);

    // Build latest items with relative modified timestamps
    let mut latest: Vec<LatestItem> = Vec::new();
    for p in pages.iter().take(5) {
        let (_, modified_rel) = format_modified(p.modified);
        latest.push(LatestItem {
            slug: p.slug.clone(),
            title: p.title.clone(),
            modified_rel,
        });
    }

    let can_edit = is_admin_cookie(jar);
    Ok(Template::render(
        "index",
        context! { latest: latest, landing_html: landing_html, pages: pages, can_edit: can_edit, metadata: metadata },
    ))
}

pub(crate) struct RemoteAddr {
    addr: String,
}

impl RemoteAddr {
    pub(crate) fn addr(&self) -> &str {
        &self.addr
    }
}

#[async_trait]
impl<'r> FromRequest<'r> for RemoteAddr {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> request::Outcome<Self, Self::Error> {
        if let Some(header) = req.headers().get_one("X-Forwarded-For") {
            if let Some(addr) = header.split(',').next() {
                return Outcome::Success(RemoteAddr {
                    addr: addr.trim().to_string(),
                });
            }
        }

        if let Some(remote) = req.remote() {
            return Outcome::Success(RemoteAddr {
                addr: remote.ip().to_string(),
            });
        }
        Outcome::Error((Status::BadRequest, ()))
    }
}

#[get("/ip")]
fn ip(req: RemoteAddr) -> String {
    let remote_ip = req.addr();
    format!("{}\n", remote_ip)
}

#[get("/tools/box-breathing")]
fn box_breathing(navigation: PageNavigation) -> Template {
    Template::render("box_breathing", context! { pages: navigation.0 })
}

fn render_page(
    page: page_store::Page,
    pages: Vec<PageListing>,
    is_admin: bool,
    conn: &mut diesel::SqliteConnection,
) -> Result<Template, Status> {
    let metadata = page_metadata(&page, conn)?;
    Ok(Template::render(
        "page",
        context! { title: page.title(), content: markdown_to_html(&page.content), pages: pages,
        metadata: metadata,
        is_admin: is_admin, slug: page.slug },
    ))
}

pub(crate) fn is_admin_cookie(jar: &CookieJar) -> bool {
    let ok_role = jar
        .get_private("user_role")
        .map(|c| c.value() == "admin")
        .unwrap_or(false);
    if !ok_role {
        return false;
    }
    if let Some(exp) = jar.get_private("session_expires") {
        if let Ok(ts) = exp.value().parse::<i64>() {
            return ts > Utc::now().timestamp();
        }
        return false;
    }
    false
}

pub(crate) fn ensure_csrf(jar: &CookieJar) -> String {
    if let Some(c) = jar.get_private("csrf") {
        c.value().to_string()
    } else {
        let token = Uuid::new_v4().to_string();
        jar.add_private(Cookie::new("csrf", token.clone()));
        token
    }
}
#[catch(401)]
fn unauthorized() -> Redirect {
    Redirect::to("/admin/login")
}

#[catch(404)]
fn page_not_found(req: &Request<'_>) -> Template {
    Template::render(
        "404",
        context! { slug: req.uri().path().as_str().trim_start_matches("/page/"), pages: Vec::<PageListing>::new() },
    )
}

// Removed admin_slash redirect — Rocket routes must not duplicate paths

fn listings_to_items(listings: &[PageListing]) -> Vec<PageItem> {
    listings
        .iter()
        .map(|p| {
            let (modified_str, modified_rel) = format_modified(p.modified);
            PageItem {
                slug: p.slug.clone(),
                title: p.title.clone(),
                modified: p.modified,
                modified_str,
                modified_rel,
            }
        })
        .collect()
}

#[get("/page")]
fn page_root(jar: &CookieJar, navigation: PageNavigation) -> Template {
    // List pages from the root pages folder and render using the same template as /pages/
    let mut pages = navigation.0;
    let all_pages = pages.clone();
    // go through pages and filter to only those in the root (no slashes in slug), then sort by modified
    pages.retain(|p| !p.slug.contains('/'));
    pages.sort_by(|a, b| b.modified.cmp(&a.modified));
    let items = listings_to_items(&pages);
    let can_edit = is_admin_cookie(jar);
    Template::render(
        "folder",
        context! { folder: "", folder_name: "Home", pages: all_pages, items: items, can_edit: can_edit },
    )
}

#[get("/page/<path..>")]
fn page_catch(
    path: PathBuf,
    jar: &CookieJar,
    navigation: PageNavigation,
    pool: &State<db::DbPool>,
) -> Result<Template, Status> {
    // join components into a slug, strip trailing ".md" and any trailing slashes
    let mut slug = path
        .iter()
        .map(|s| s.to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    // Trim any trailing slash chars
    while slug.ends_with('/') {
        slug.pop();
    }
    if let Some(s) = slug.strip_suffix(".md") {
        slug = s.to_string();
    }
    if slug.is_empty() {
        // Shouldn't happen because /page is a separate route, but handle defensively
        return Err(Status::NotFound);
    }
    let admin = is_admin_cookie(jar);
    let mut conn = pool.get().map_err(page_error)?;
    if let Some(page) = page_store::by_slug(&mut conn, &slug).map_err(page_error)? {
        if page.hidden && !admin {
            return Err(Status::NotFound);
        }
        return render_page(page, navigation.0, admin, &mut conn);
    }
    let prefix = format!("{slug}/");
    let folder_pages: Vec<_> = navigation
        .0
        .iter()
        .filter(|p| p.slug.starts_with(&prefix))
        .cloned()
        .collect();
    if !folder_pages.is_empty() {
        let items = listings_to_items(&folder_pages);
        let folder_name = slug
            .split('/')
            .last()
            .unwrap_or(slug.as_str())
            .replace('-', " ");
        let pages = navigation.0;
        return Ok(Template::render(
            "folder",
            context! { folder: &slug, folder_name: folder_name, pages: pages, items: items, can_edit: admin },
        ));
    }

    Err(Status::NotFound)
}

#[get("/pages?<page>")]
fn pages_index(page: Option<usize>, navigation: PageNavigation) -> Template {
    let pages = navigation.0;
    let total = pages.len();
    let per_page: usize = 20;
    let total_pages = if total == 0 {
        1
    } else {
        (total + per_page - 1) / per_page
    };
    let mut cur = page.unwrap_or(1);
    if cur < 1 {
        cur = 1
    }
    if cur > total_pages {
        cur = total_pages
    }

    let start = (cur - 1).saturating_mul(per_page);
    let end = std::cmp::min(start + per_page, total);
    let items: Vec<PageItem> = if start >= end {
        Vec::new()
    } else {
        listings_to_items(&pages[start..end])
    };

    let page_numbers: Vec<usize> = (1..=total_pages).collect();

    Template::render(
        "pages",
        context! { pages: pages, items: items, current_page: cur, total_pages: total_pages, page_numbers: page_numbers },
    )
}

#[rocket::main]
async fn main() -> Result<(), rocket::Error> {
    // Initialize DB pool and run embedded migrations
    let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| "wasd5.db".to_string());
    let pool = db::establish_pool(&database_url);
    {
        let mut conn = pool
            .get()
            .expect("Failed to get DB connection for migrations");
        db::run_migrations(&mut conn).expect("Failed to run migrations");
    }

    let fig = rocket::build()
        .manage(pool)
        .manage(
            comics::ComicStorage::initialize(
                env::var_os("COMICS_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("uploads").join("comics")),
            )
            .expect("Failed to initialize comics image storage"),
        )
        .mount("/", comics::routes())
        .mount(
            "/",
            routes![
                index,
                admin_landing_get,
                admin_landing_post,
                admin_upload_file,
                admin_upload_picture,
                admin_create_picture_folder,
                admin_rename_move_picture,
                admin_update_calendar_allowed_ips,
                ip,
                box_breathing,
                page_root,
                pages_index,
                page_catch,
                create_user,
                list_users,
                admin_login_get,
                admin_login_post,
                admin_logout,
                admin_index,
                admin_users,
                admin_users_new,
                admin_users_create,
                admin_users_update,
                admin_edit_page_get,
                admin_edit_page_post,
                admin_files_get,
                admin_pictures_get,
                admin_calendar_settings_get,
                admin_pages_new_get,
                admin_pages_new_post,
                calendar_index,
                api_calendar_access,
                api_calendar_get,
                api_calendar_create,
                api_calendar_update,
                api_calendar_delete,
                list_packages,
                create_package,
                delete_package,
                mark_received,
                update_tracking,
                creatine_index,
                creatine_log,
                creatine_delete,
                creatine_reminder_toggle
            ],
        )
        .mount("/static", FileServer::from(STATIC_DIR))
        .register("/", catchers![unauthorized, page_not_found])
        .attach(Template::custom(|engines| {
            engines.tera.register_filter("category", category_filter);
        }));

    fig.launch().await?;
    Ok(())
}

#[cfg(test)]
mod page_tests {
    use super::*;
    use diesel::prelude::*;
    use rocket::http::ContentType;
    use rocket::local::blocking::Client;

    fn client() -> (Client, tempfile::TempDir, db::DbPool) {
        let dir = tempfile::tempdir().unwrap();
        let pool = db::establish_pool(dir.path().join("test.db").to_str().unwrap());
        {
            let mut conn = pool.get().unwrap();
            db::run_migrations(&mut conn).unwrap();
            page_store::create(&mut conn, "visible", "# Visible article", "Alice", false).unwrap();
            page_store::create(&mut conn, "drafts/secret", "# SECRET_DRAFT", "Alice", true)
                .unwrap();
            page_store::create(
                &mut conn,
                page_store::LANDING_SLUG,
                "# SECRET_HOME",
                "Alice",
                true,
            )
            .unwrap();
            let password_hash = bcrypt::hash("password", 4).unwrap();
            diesel::insert_into(schema::users::table)
                .values(models::NewUser {
                    username: "admin",
                    password_hash: &password_hash,
                    role: "admin",
                    created_at: Utc::now().timestamp(),
                    email: None,
                })
                .execute(&mut conn)
                .unwrap();
        }
        let app = rocket::custom(rocket::Config::debug_default())
            .manage(pool.clone())
            .mount(
                "/",
                routes![
                    index,
                    box_breathing,
                    page_catch,
                    page_root,
                    pages_index,
                    admin_login_get,
                    admin_login_post,
                    admin_logout,
                    admin_index,
                    admin_pages_new_get,
                    admin_pages_new_post,
                    admin_edit_page_get,
                    admin_edit_page_post,
                    admin_landing_get,
                    admin_landing_post
                ],
            )
            .register("/", catchers![unauthorized, page_not_found])
            .attach(Template::custom(|engines| {
                engines.tera.register_filter("category", category_filter);
            }));
        (Client::tracked(app).unwrap(), dir, pool)
    }

    fn login(client: &Client) -> String {
        assert_eq!(
            client
                .post("/admin/login")
                .header(ContentType::Form)
                .body("username=admin&password=password")
                .dispatch()
                .status(),
            Status::SeeOther
        );
        let response = client.get("/admin/pages/new").dispatch();
        assert_eq!(response.status(), Status::Ok);
        response
            .into_string()
            .unwrap()
            .split("name=\"csrf\" value=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .to_string()
    }

    #[test]
    fn breathing_tool_and_health_menu() {
        let (client, _dir, _pool) = client();
        let response = client.get("/tools/box-breathing").dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body = response.into_string().unwrap();
        assert!(body.contains("Health"));
        assert!(body.contains("href=\"/tools/box-breathing\""));
        assert!(body.contains("Box breathing"));
        assert!(body.contains("Box (4-4-4-4)"));
        assert!(body.contains("4-7-8 triangular breathing guide"));
        assert!(body.contains("Hold · 7s"));
        assert!(body.contains("Exhale · 8s"));
        assert!(body.contains("/static/box-breathing.css"));
        assert!(body.contains("/static/box-breathing.js"));
    }

    #[test]
    fn sql_views_metadata_and_draft_visibility() {
        let (client, _dir, _pool) = client();
        for uri in ["/", "/pages", "/page", "/page/visible", "/page/visible.md"] {
            let response = client.get(uri).dispatch();
            assert_eq!(response.status(), Status::Ok, "{uri}");
            let body = response.into_string().unwrap();
            assert!(!body.contains("SECRET_DRAFT"), "{uri}");
            assert!(!body.contains("SECRET_HOME"), "{uri}");
            if uri == "/page/visible" {
                assert!(body.contains("Created:"));
                assert!(body.contains("by Alice"));
                assert!(body.contains("Editors: Alice"));
                assert!(body.contains("Last updated:"));
            }
        }
        for uri in [
            "/page/drafts/secret",
            "/page/drafts",
            "/admin/pages/edit/drafts/secret",
        ] {
            let status = client.get(uri).dispatch().status();
            assert!(
                status == Status::NotFound || status == Status::SeeOther,
                "{uri}: {status}"
            );
        }
        login(&client);
        for uri in [
            "/admin",
            "/page/drafts",
            "/page/drafts/secret",
            "/admin/pages/edit/drafts/secret",
            "/admin/landing",
        ] {
            let response = client.get(uri).dispatch();
            assert_eq!(response.status(), Status::Ok, "{uri}");
            let body = response.into_string().unwrap();
            assert!(
                body.contains("SECRET_DRAFT") || body.contains("SECRET_HOME"),
                "{uri}"
            );
        }
        let body = client
            .get("/page/drafts/secret")
            .dispatch()
            .into_string()
            .unwrap();
        assert!(body.contains("Hidden draft"));
        let body = client
            .get("/admin/pages/edit/drafts/secret")
            .dispatch()
            .into_string()
            .unwrap();
        assert!(body.contains("History"));
        assert!(body.contains("by Alice at"));
    }

    #[test]
    fn browser_edits_history_conflicts_and_landing() {
        let (client, _dir, pool) = client();
        let csrf = login(&client);
        let response = client
            .post("/admin/pages/new")
            .header(ContentType::Form)
            .body(format!(
                "slug=new-draft&content=%23+New&hidden=on&csrf={csrf}"
            ))
            .dispatch();
        assert_eq!(response.status(), Status::SeeOther);
        let mut conn = pool.get().unwrap();
        let page = page_store::by_slug(&mut conn, "new-draft")
            .unwrap()
            .unwrap();
        assert!(page.hidden);
        assert_eq!(page.creator, "admin");
        let revision = page_store::latest_revision(&mut conn, page.id).unwrap();
        let response = client
            .post("/admin/pages/new")
            .header(ContentType::Form)
            .body(format!("slug=new-draft&content=overwrite&csrf={csrf}"))
            .dispatch();
        assert_eq!(response.status(), Status::Conflict);
        let response = client
            .post("/admin/pages/edit/new-draft")
            .header(ContentType::Form)
            .body(format!(
                "content=%23+Published&revision={revision}&csrf={csrf}"
            ))
            .dispatch();
        assert_eq!(response.status(), Status::SeeOther);
        assert!(!page_store::by_id(&mut conn, page.id).unwrap().hidden);
        assert_eq!(page_store::history(&mut conn, page.id).unwrap().len(), 2);
        let response = client
            .post("/admin/pages/edit/new-draft")
            .header(ContentType::Form)
            .body(format!("content=stale&revision={revision}&csrf={csrf}"))
            .dispatch();
        assert_eq!(response.status(), Status::Conflict);
        assert!(
            response
                .into_string()
                .unwrap()
                .contains("Page changed since")
        );
        assert_eq!(
            page_store::by_id(&mut conn, page.id).unwrap().content,
            "# Published"
        );
        assert_eq!(
            client
                .post("/admin/pages/edit/new-draft")
                .header(ContentType::Form)
                .body(format!("content=bad&revision={revision}&csrf=wrong"))
                .dispatch()
                .status(),
            Status::Forbidden
        );
        let landing = page_store::by_slug(&mut conn, page_store::LANDING_SLUG)
            .unwrap()
            .unwrap();
        let revision = page_store::latest_revision(&mut conn, landing.id).unwrap();
        assert_eq!(
            client
                .post("/admin/landing")
                .header(ContentType::Form)
                .body(format!(
                    "content=%23+PUBLIC_HOME&revision={revision}&csrf={csrf}"
                ))
                .dispatch()
                .status(),
            Status::SeeOther
        );
        assert_eq!(
            page_store::by_id(&mut conn, landing.id).unwrap().content,
            "# PUBLIC_HOME"
        );
        assert_eq!(
            page_store::history(&mut conn, landing.id).unwrap()[0].editor,
            "admin"
        );
        client.get("/admin/logout").dispatch();
        let body = client.get("/").dispatch().into_string().unwrap();
        assert!(body.contains("PUBLIC_HOME"));
        assert!(!body.contains("SECRET_DRAFT"));
        assert_eq!(
            client.get("/page/new-draft").dispatch().status(),
            Status::Ok
        );
        assert_eq!(
            client.get("/page/drafts/secret").dispatch().status(),
            Status::NotFound
        );
    }
}
