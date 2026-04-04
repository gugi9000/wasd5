use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;
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
use rocket::response::status::NotFound;
use rocket::{get, routes};
use rocket_dyn_templates::tera;
use rocket_dyn_templates::{Template, context};
use serde::Serialize;
use uuid::Uuid;

mod db;
mod helpers;
mod models;
mod schema;
mod calendar;
mod admin;
use admin::{
    admin_calendar_settings_get, admin_edit_page_get, admin_edit_page_post, admin_files_get,
    admin_index, admin_landing_get, admin_landing_post, admin_login_get, admin_login_post,
    admin_logout, admin_pages_new_get, admin_pages_new_post, admin_pictures_get,
    admin_upload_file, admin_upload_picture, admin_users, admin_users_create, admin_users_new,
    admin_update_calendar_allowed_ips, create_user, list_users,
};
use calendar::{
    api_calendar_access, api_calendar_create, api_calendar_delete, api_calendar_get,
    api_calendar_update,
    calendar_index,
};
use helpers::format_modified;

pub(crate) const PAGES_DIR: &str = "pages";
const STATIC_DIR: &str = "static";

#[derive(Serialize, Clone)]
pub(crate) struct PageListing {
    slug: String,
    title: String,
    modified: u64,
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

fn collect_pages(dir: &std::path::Path, base: &std::path::Path, pages: &mut Vec<PageListing>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            collect_pages(&path, base, pages);
        } else if path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        {
            if let (Some(stem), Ok(rel)) = (
                path.file_stem().and_then(|s| s.to_str()),
                path.strip_prefix(base),
            ) {
                let slug = rel.with_extension("").to_string_lossy().replace('\\', "/");
                let title = stem.replace('-', " ");
                let modified = match path.metadata().and_then(|m| m.modified()) {
                    Ok(t) => t
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                    Err(_) => 0,
                };
                pages.push(PageListing {
                    slug,
                    title,
                    modified,
                });
            }
        }
    }
}

pub(crate) fn read_pages() -> Vec<PageListing> {
    // recursively reads PAGES_DIR and returns the pages found, with slug (path without extension), title (from filename), and modified timestamp for sorting
    let mut pages = Vec::new();
    let dir = std::path::Path::new(PAGES_DIR);
    collect_pages(dir, dir, &mut pages);
    // newest first
    pages.sort_by(|a, b| b.modified.cmp(&a.modified));
    pages
}

#[get("/")]
fn index(jar: &CookieJar) -> Template {
    let pages = read_pages();
    // Landing content stored in pages/landing/landing.md — fall back to a default welcome.
    let mut landing_md = String::from(
        "# Welcome\n\nWelcome to the simple Rocket + Tera site serving Markdown pages.",
    );
    let mut path = PathBuf::from(PAGES_DIR);
    path.push("landing/landing.md");
    if let Ok(s) = fs::read_to_string(&path) {
        if !s.trim().is_empty() {
            landing_md = s;
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
    Template::render(
        "index",
        context! { latest: latest, landing_html: landing_html, pages: pages, can_edit: can_edit },
    )
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


fn render_page(slug: &str, is_admin: bool) -> Result<Template, NotFound<Template>> {
    let mut path = PathBuf::from(PAGES_DIR);
    path.push(format!("{}.md", slug));
    if !path.exists() {
        return Err(NotFound(Template::render("404", context! { slug: slug })));
    }
    let md = fs::read_to_string(&path)
        .map_err(|_| NotFound(Template::render("404", context! { slug: slug })))?;
    let html = markdown_to_html(&md);
    // try to extract title from first heading, otherwise use slug
    let title = md
        .lines()
        .find_map(|l| {
            let t = l.trim();
            if t.starts_with('#') {
                Some(t.trim_start_matches('#').trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| slug.replace('-', " "));

    let pages = read_pages();

    // compute last-updated for this page from file metadata
    let mut last_updated = String::new();
    if let Ok(meta) = std::fs::metadata(&path) {
        if let Ok(modtime) = meta.modified() {
            if let Ok(dur) = modtime.duration_since(SystemTime::UNIX_EPOCH) {
                let (abs, rel) = format_modified(dur.as_secs());
                if !abs.is_empty() {
                    last_updated = format!("{} ({})", abs, rel);
                }
            }
        }
    }

    Ok(Template::render(
        "page",
        context! { title: title, content: html, pages: pages, last_updated: last_updated, is_admin: is_admin, slug: slug },
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
fn page_root(jar: &CookieJar) -> Result<Template, NotFound<Template>> {
    // List pages from the root pages folder and render using the same template as /pages/
    let mut pages = read_pages();
    let all_pages = pages.clone();
    // go through pages and filter to only those in the root (no slashes in slug), then sort by modified
    pages.retain(|p| !p.slug.contains('/'));
    pages.sort_by(|a, b| b.modified.cmp(&a.modified));
    let items = listings_to_items(&pages);
    let can_edit = is_admin_cookie(jar);
    Ok(Template::render(
        "folder",
        context! { folder: "", folder_name: "Home", pages: all_pages, items: items, can_edit: can_edit },
    ))

}


#[get("/page/<path..>")]
fn page_catch(path: std::path::PathBuf, jar: &CookieJar) -> Result<Template, NotFound<Template>> {
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
        return Err(NotFound(Template::render("404", context! { slug: "" })));
    }

    // Check if slug resolves to a folder in the pages directory
    let folder_path = PathBuf::from(PAGES_DIR).join(&slug);
    if folder_path.is_dir() {
        let mut folder_pages: Vec<PageListing> = Vec::new();
        collect_pages(&folder_path, &PathBuf::from(PAGES_DIR), &mut folder_pages);
        folder_pages.sort_by(|a, b| b.modified.cmp(&a.modified));
        let items = listings_to_items(&folder_pages);
        let folder_name = slug.split('/').last().unwrap_or(slug.as_str()).replace('-', " ");
        let pages = read_pages();
        return Ok(Template::render(
            "folder",
            context! { folder: &slug, folder_name: folder_name, pages: pages, items: items },
        ));
    }

    let admin = is_admin_cookie(jar);
    render_page(&slug, admin)
}

#[get("/pages?<page>")]
fn pages_index(page: Option<usize>) -> Template {
    let pages = read_pages();
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
        if let Err(e) = db::run_migrations(&mut conn) {
            eprintln!("Failed to run migrations: {}", e);
        } else {
            println!("Migrations applied");
        }
    }

    let fig = rocket::build()
        .manage(pool)
        .mount(
            "/",
            routes![
                index,
                admin_landing_get,
                admin_landing_post,
                admin_upload_file,
                admin_upload_picture,
                admin_update_calendar_allowed_ips,
                ip,
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
                api_calendar_delete
            ],
        )
        .mount("/static", FileServer::from(STATIC_DIR))
        .register("/", catchers![unauthorized])
        .attach(Template::custom(|engines| {
            engines.tera.register_filter("category", category_filter);
        }));

    fig.launch().await?;
    Ok(())
}
