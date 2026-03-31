use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::{collections::HashMap, env};

use chrono::Utc;
use pulldown_cmark::{Options, Parser, html};
use rocket::FromForm;
use rocket::async_trait;
use rocket::catch;
use rocket::catchers;
use rocket::fs::{FileServer, TempFile};
use rocket::form::Form;
use rocket::http::Status;
use rocket::http::{Cookie, CookieJar};
use rocket::request::{self, FromRequest, Outcome, Request};
use rocket::response::Redirect;
use rocket::response::status::NotFound;
use rocket::serde::{Deserialize, json::Json};
use rocket::{State, get, post, routes};
use rocket_dyn_templates::tera;
use rocket_dyn_templates::{Template, context};
use serde::Serialize;
use uuid::Uuid;

mod db;
mod helpers;
mod models;
mod schema;
mod calendar;
use bcrypt::{DEFAULT_COST, hash, verify};
use calendar::{
    api_calendar_create, api_calendar_delete, api_calendar_get, api_calendar_update,
    calendar_index,
};
use helpers::format_modified;
use db::DbPool;
use diesel::prelude::*;

const PAGES_DIR: &str = "pages";
const STATIC_DIR: &str = "static";
const STATIC_FILES_DIR: &str = "static/files";
const STATIC_PICTURES_DIR: &str = "static/pictures";

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

#[derive(Serialize)]
struct AssetItem {
    name: String,
    url: String,
    markdown: String,
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

struct RemoteAddr {
    addr: String,
}

impl RemoteAddr {
    fn addr(self) -> String {
        self.addr
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

fn sanitize_upload_filename(input: &str) -> String {
    let base = Path::new(input)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("upload.bin");
    let mut out = String::with_capacity(base.len());
    for ch in base.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
            out.push(ch);
        } else if ch.is_ascii_whitespace() {
            out.push('-');
        }
    }
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    out.trim_matches('.').trim_matches('-').to_string()
}

fn unique_upload_path(dir: &Path, filename: &str) -> PathBuf {
    let initial = dir.join(filename);
    if !initial.exists() {
        return initial;
    }

    let stem = Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("upload");
    let ext = Path::new(filename)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    for i in 1..10_000 {
        let candidate_name = if ext.is_empty() {
            format!("{}-{}", stem, i)
        } else {
            format!("{}-{}.{}", stem, i, ext)
        };
        let candidate = dir.join(candidate_name);
        if !candidate.exists() {
            return candidate;
        }
    }

    dir.join(format!("upload-{}", Utc::now().timestamp()))
}

fn is_allowed_picture_filename(filename: &str) -> bool {
    let ext = Path::new(filename)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "avif"
    )
}

fn list_static_assets(dir: &str, url_prefix: &str, as_image_markdown: bool) -> Vec<AssetItem> {
    let mut items = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return items,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let url = format!("{}/{}", url_prefix, name);
        let markdown = if as_image_markdown {
            format!("![]({})", url)
        } else {
            format!("[{}]({})", name, url)
        };
        items.push(AssetItem {
            name: name.to_string(),
            url,
            markdown,
        });
    }

    items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    items
}

#[derive(FromForm)]
struct LoginForm {
    username: String,
    password: String,
}

#[get("/admin/login")]
fn admin_login_get(jar: &CookieJar) -> Template {
    let pages = read_pages();
    let csrf = ensure_csrf(jar);
    Template::render("admin/login", context! { pages: pages, csrf: csrf })
}

#[post("/admin/login", data = "<form>")]
fn admin_login_post(
    form: Form<LoginForm>,
    jar: &CookieJar,
    pool: &State<DbPool>,
) -> Result<Redirect, Template> {
    use crate::schema::users::dsl::*;
    let f = form.into_inner();
    let mut conn = pool.get().map_err(|_| {
        let pages = read_pages();
        Template::render(
            "admin/login",
            context! { error: "DB unavailable", pages: pages },
        )
    })?;
    let user_opt = users
        .filter(username.eq(&f.username))
        .first::<models::User>(&mut conn)
        .optional()
        .map_err(|_| Template::render("admin/login", context! { error: "DB error" }))?;
    if let Some(u) = user_opt {
        if verify(&f.password, &u.password_hash).unwrap_or(false) {
            jar.add_private(Cookie::new("user_id", u.id.to_string()));
            jar.add_private(Cookie::new("username", u.username.clone()));
            jar.add_private(Cookie::new("user_role", u.role.clone()));
            let expiry = Utc::now().timestamp() + 24 * 3600;
            jar.add_private(Cookie::new("session_expires", expiry.to_string()));
            return Ok(Redirect::to("/admin"));
        }
    }
    let pages = read_pages();
    let csrf = ensure_csrf(jar);
    Err(Template::render(
        "admin/login",
        context! { error: "Invalid credentials", pages: pages, csrf: csrf },
    ))
}

#[get("/admin/logout")]
fn admin_logout(jar: &CookieJar) -> Redirect {
    jar.remove_private(Cookie::new("user_id", ""));
    jar.remove_private(Cookie::new("username", ""));
    jar.remove_private(Cookie::new("user_role", ""));
    jar.remove_private(Cookie::new("session_expires", ""));
    Redirect::to("/admin/login")
}

#[get("/admin?<warning>")]
fn admin_index(jar: &CookieJar, warning: Option<&str>) -> Result<Template, Redirect> {
    if !is_admin_cookie(jar) {
        return Err(Redirect::to("/admin/login"));
    }
    let pages = read_pages();
    let csrf = ensure_csrf(jar);
    let files = list_static_assets(STATIC_FILES_DIR, "/static/files", false);
    let pictures = list_static_assets(STATIC_PICTURES_DIR, "/static/pictures", true);
    Ok(Template::render(
        "admin/index",
        context! { pages: pages, csrf: csrf, files: files, pictures: pictures, warning: warning },
    ))
}

#[post("/admin/upload/file", data = "<form>")]
async fn admin_upload_file(jar: &CookieJar<'_>, form: Form<UploadForm<'_>>) -> Redirect {
    if !is_admin_cookie(jar) {
        return Redirect::to("/admin/login");
    }

    let mut f = form.into_inner();
    if let Some(form_csrf) = f.csrf.as_ref() {
        if let Some(cookie_csrf) = jar.get_private("csrf") {
            if cookie_csrf.value() != form_csrf.as_str() {
                return Redirect::to("/admin");
            }
        } else {
            return Redirect::to("/admin");
        }
    } else {
        return Redirect::to("/admin");
    }

    let incoming = f
        .upload
        .raw_name()
        .map(|n| n.dangerous_unsafe_unsanitized_raw().to_string())
        .unwrap_or_else(|| "upload.bin".to_string());
    let original_name = Path::new(&incoming)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("upload.bin")
        .to_string();
    let mut filename = sanitize_upload_filename(&incoming);
    let sanitized_changed = filename != original_name;
    if filename.is_empty() {
        filename = format!("upload-{}.bin", Utc::now().timestamp());
    }

    if fs::create_dir_all(STATIC_FILES_DIR).is_err() {
        return Redirect::to("/admin");
    }
    let target = unique_upload_path(Path::new(STATIC_FILES_DIR), &filename);
    if f.upload.persist_to(&target).await.is_err() {
        return Redirect::to("/admin");
    }

    if sanitized_changed {
        Redirect::to("/admin?warning=filename_sanitized")
    } else {
        Redirect::to("/admin")
    }
}

#[post("/admin/upload/picture", data = "<form>")]
async fn admin_upload_picture(jar: &CookieJar<'_>, form: Form<UploadForm<'_>>) -> Redirect {
    if !is_admin_cookie(jar) {
        return Redirect::to("/admin/login");
    }

    let mut f = form.into_inner();
    if let Some(form_csrf) = f.csrf.as_ref() {
        if let Some(cookie_csrf) = jar.get_private("csrf") {
            if cookie_csrf.value() != form_csrf.as_str() {
                return Redirect::to("/admin");
            }
        } else {
            return Redirect::to("/admin");
        }
    } else {
        return Redirect::to("/admin");
    }

    let incoming = f
        .upload
        .raw_name()
        .map(|n| n.dangerous_unsafe_unsanitized_raw().to_string())
        .unwrap_or_else(|| "image.bin".to_string());
    let original_name = Path::new(&incoming)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("image.bin")
        .to_string();
    let mut filename = sanitize_upload_filename(&incoming);
    let sanitized_changed = filename != original_name;
    if filename.is_empty() {
        filename = format!("image-{}.bin", Utc::now().timestamp());
    }

    let content_type = f
        .upload
        .content_type()
        .map(|ct| ct.to_string())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let is_image_content = content_type.starts_with("image/");
    if !is_image_content {
        return Redirect::to("/admin");
    }

    if !is_allowed_picture_filename(&filename)
        && Path::new(&filename).extension().is_none()
        && !content_type.is_empty()
    {
        let subtype = content_type
            .split('/')
            .nth(1)
            .unwrap_or("img")
            .split(';')
            .next()
            .unwrap_or("img");
        let safe_subtype = subtype
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>();
        let ext = if safe_subtype.is_empty() {
            "img"
        } else {
            safe_subtype.as_str()
        };
        filename = format!("{}.{}", filename, ext);
    }

    if fs::create_dir_all(STATIC_PICTURES_DIR).is_err() {
        return Redirect::to("/admin");
    }
    let target = unique_upload_path(Path::new(STATIC_PICTURES_DIR), &filename);
    if f.upload.persist_to(&target).await.is_err() {
        return Redirect::to("/admin");
    }

    if sanitized_changed {
        Redirect::to("/admin?warning=filename_sanitized")
    } else {
        Redirect::to("/admin")
    }
}

#[get("/admin/users")]
fn admin_users(jar: &CookieJar, pool: &State<DbPool>) -> Result<Template, Redirect> {
    if !is_admin_cookie(jar) {
        return Err(Redirect::to("/admin/login"));
    }
    use crate::schema::users::dsl::{created_at, users};
    let mut conn = match pool.get() {
        Ok(c) => c,
        Err(_) => {
            return Ok(Template::render(
                "admin/users",
                context! { error: "DB unavailable", users: Vec::<models::User>::new() },
            ));
        }
    };
    let results = users
        .order(created_at.desc())
        .load::<models::User>(&mut conn)
        .unwrap_or_default();
    let pages = read_pages();
    Ok(Template::render(
        "admin/users",
        context! { users: results, pages: pages },
    ))
}

#[get("/admin/users/new")]
fn admin_users_new(jar: &CookieJar) -> Result<Template, Redirect> {
    if !is_admin_cookie(jar) {
        return Err(Redirect::to("/admin/login"));
    }
    let pages = read_pages();
    let csrf = ensure_csrf(jar);
    Ok(Template::render(
        "admin/new_user",
        context! { pages: pages, csrf: csrf },
    ))
}

#[derive(FromForm)]
struct NewUserForm {
    username: String,
    password: String,
    role: Option<String>,
    // Include CSRF in the form so we can validate it against the private cookie
    csrf: Option<String>,
}

#[derive(FromForm)]
struct LandingForm {
    content: String,
    csrf: Option<String>,
}

#[derive(FromForm)]
struct UploadForm<'r> {
    upload: TempFile<'r>,
    csrf: Option<String>,
}

#[get("/admin/landing")]
fn admin_landing_get(jar: &CookieJar) -> Result<Template, Redirect> {
    if !is_admin_cookie(jar) {
        return Err(Redirect::to("/admin/login"));
    }
    let pages = read_pages();
    let mut landing_md = String::from("# Welcome\n\nWelcome to the site.");
    let mut path = PathBuf::from(PAGES_DIR);
    path.push("landing.md");
    if let Ok(s) = fs::read_to_string(&path) {
        landing_md = s;
    }
    let csrf = ensure_csrf(jar);
    Ok(Template::render(
        "admin/landing_edit",
        context! { pages: pages, content: landing_md, csrf: csrf },
    ))
}

#[post("/admin/landing", data = "<form>")]
fn admin_landing_post(jar: &CookieJar, form: Form<LandingForm>) -> Redirect {
    if !is_admin_cookie(jar) {
        return Redirect::to("/admin/login");
    }
    let f = form.into_inner();
    // CSRF validation
    if let Some(form_csrf) = f.csrf.as_ref() {
        if let Some(cookie_csrf) = jar.get_private("csrf") {
            if cookie_csrf.value() != form_csrf.as_str() {
                return Redirect::to("/admin/landing");
            }
        } else {
            return Redirect::to("/admin/landing");
        }
    } else {
        return Redirect::to("/admin/landing");
    }

    let mut path = PathBuf::from(PAGES_DIR);
    if !path.exists() {
        let _ = std::fs::create_dir_all(&path);
    }
    path.push("landing.md");
    let _ = fs::write(&path, f.content.as_bytes());
    Redirect::to("/")
}

#[post("/admin/users", data = "<form>")]
fn admin_users_create(jar: &CookieJar, pool: &State<DbPool>, form: Form<NewUserForm>) -> Redirect {
    if !is_admin_cookie(jar) {
        return Redirect::to("/admin/login");
    }
    use crate::schema::users::dsl::users;
    let f = form.into_inner();
    // Validate CSRF token from form against private cookie
    if let Some(form_csrf) = f.csrf.as_ref() {
        if let Some(cookie_csrf) = jar.get_private("csrf") {
            if cookie_csrf.value() != form_csrf.as_str() {
                return Redirect::to("/admin/users");
            }
        } else {
            return Redirect::to("/admin/users");
        }
    } else {
        return Redirect::to("/admin/users");
    }

    let role_val = f.role.unwrap_or_else(|| "member".to_string());
    let pw_hash = hash(&f.password, DEFAULT_COST).unwrap_or_else(|_| "".to_string());
    let new = models::NewUser {
        username: f.username.as_str(),
        password_hash: pw_hash.as_str(),
        role: role_val.as_str(),
        created_at: Utc::now().timestamp(),
    };
    if let Ok(mut conn) = pool.get() {
        let _ = diesel::insert_into(users).values(&new).execute(&mut conn);
    }
    Redirect::to("/admin/users")
}

#[derive(FromForm)]
struct EditPageForm {
    content: String,
    csrf: Option<String>,
}

#[derive(FromForm)]
struct NewPageForm {
    slug: String,
    content: String,
    csrf: Option<String>,
}

#[get("/admin/pages/new")]
fn admin_pages_new_get(jar: &CookieJar) -> Result<Template, Redirect> {
    if !is_admin_cookie(jar) {
        return Err(Redirect::to("/admin/login"));
    }
    let pages = read_pages();
    let csrf = ensure_csrf(jar);
    Ok(Template::render(
        "admin/new_page",
        context! { pages: pages, csrf: csrf },
    ))
}

#[post("/admin/pages/new", data = "<form>")]
fn admin_pages_new_post(jar: &CookieJar, form: Form<NewPageForm>) -> Redirect {
    if !is_admin_cookie(jar) {
        return Redirect::to("/admin/login");
    }
    let f = form.into_inner();
    // CSRF validation
    if let Some(form_csrf) = f.csrf.as_ref() {
        if let Some(cookie_csrf) = jar.get_private("csrf") {
            if cookie_csrf.value() != form_csrf.as_str() {
                return Redirect::to("/admin/pages/new");
            }
        } else {
            return Redirect::to("/admin/pages/new");
        }
    } else {
        return Redirect::to("/admin/pages/new");
    }

    // normalize slug
    let mut slug = f.slug.trim().to_string();
    if slug.ends_with('/') {
        slug.pop();
    }
    if slug.ends_with(".md") {
        slug = slug.trim_end_matches(".md").to_string();
    }
    if slug.is_empty() {
        return Redirect::to("/admin/pages/new");
    }
    if slug.contains("..") {
        return Redirect::to("/admin/pages/new");
    }

    // convert spaces to dashes for filename component
    slug = slug
        .split('/')
        .map(|s| s.trim().replace(' ', "-"))
        .collect::<Vec<_>>()
        .join("/");

    let mut page_path = PathBuf::from(PAGES_DIR);
    for comp in slug.split('/') {
        page_path.push(comp);
    }
    if let Some(parent) = page_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    page_path.set_extension("md");

    if let Err(_) = fs::write(&page_path, f.content.as_bytes()) {
        return Redirect::to("/admin/pages/new");
    }

    Redirect::to(format!("/admin/pages/edit/{}", slug))
}

#[get("/admin/pages/edit/<path..>")]
fn admin_edit_page_get(path: std::path::PathBuf, jar: &CookieJar) -> Result<Template, Redirect> {
    if !is_admin_cookie(jar) {
        return Err(Redirect::to("/admin/login"));
    }
    let mut slug = path
        .iter()
        .map(|s| s.to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    while slug.ends_with('/') {
        slug.pop();
    }
    if slug.is_empty() {
        return Err(Redirect::to("/admin"));
    }
    let mut page_path = PathBuf::from(PAGES_DIR);
    for comp in slug.split('/') {
        page_path.push(comp);
    }
    page_path.set_extension("md");
    let content = fs::read_to_string(&page_path).unwrap_or_else(|_| String::new());
    let pages = read_pages();
    let csrf = ensure_csrf(jar);
    Ok(Template::render(
        "admin/edit_page",
        context! { pages: pages, slug: slug, content: content, csrf: csrf },
    ))
}

#[post("/admin/pages/edit/<path..>", data = "<form>")]
fn admin_edit_page_post(
    path: std::path::PathBuf,
    jar: &CookieJar,
    form: Form<EditPageForm>,
) -> Redirect {
    if !is_admin_cookie(jar) {
        return Redirect::to("/admin/login");
    }
    // CSRF validation
    let f = form.into_inner();
    if let Some(form_csrf) = f.csrf.as_ref() {
        if let Some(cookie_csrf) = jar.get_private("csrf") {
            if cookie_csrf.value() != form_csrf.as_str() {
                return Redirect::to("/admin");
            }
        } else {
            return Redirect::to("/admin");
        }
    } else {
        return Redirect::to("/admin");
    }

    let mut slug = path
        .iter()
        .map(|s| s.to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    while slug.ends_with('/') {
        slug.pop();
    }
    if slug.is_empty() {
        return Redirect::to("/admin");
    }

    let mut page_path = PathBuf::from(PAGES_DIR);
    for comp in slug.split('/') {
        page_path.push(comp);
    }
    // ensure parent dir exists
    if let Some(parent) = page_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    page_path.set_extension("md");

    // Write content
    if let Err(_) = fs::write(&page_path, f.content.as_bytes()) {
        return Redirect::to(format!("/admin/pages/edit/{}", slug));
    }

    Redirect::to(format!("/page/{}", slug))
}

// Removed admin_slash redirect — Rocket routes must not duplicate paths

#[catch(401)]
fn unauthorized() -> Redirect {
    Redirect::to("/admin/login")
}

#[derive(Deserialize)]
struct CreateUser {
    username: String,
    password: String,
    role: Option<String>,
}

#[post("/api/admin/users", data = "<payload>")]
fn create_user(
    jar: &CookieJar,
    pool: &State<DbPool>,
    payload: Json<CreateUser>,
) -> Result<String, Status> {
    if !is_admin_cookie(jar) {
        return Err(Status::Unauthorized);
    }
    use crate::schema::users::dsl::*;
    let role_val = payload.role.clone().unwrap_or_else(|| "member".to_string());
    let pw_hash = hash(&payload.password, DEFAULT_COST).map_err(|_| Status::InternalServerError)?;
    let new = models::NewUser {
        username: &payload.username,
        password_hash: &pw_hash,
        role: &role_val,
        created_at: Utc::now().timestamp(),
    };

    let mut conn = pool.get().map_err(|_| Status::ServiceUnavailable)?;
    diesel::insert_into(users)
        .values(&new)
        .execute(&mut conn)
        .map_err(|_| Status::InternalServerError)?;

    Ok(format!("created user {}", payload.username))
}

#[get("/api/admin/users")]
fn list_users(jar: &CookieJar, pool: &State<DbPool>) -> Result<Json<Vec<models::User>>, Status> {
    if !is_admin_cookie(jar) {
        return Err(Status::Unauthorized);
    }
    use crate::schema::users::dsl::*;
    let mut conn = pool.get().map_err(|_| Status::ServiceUnavailable)?;
    let results = users
        .order(created_at.desc())
        .load::<models::User>(&mut conn)
        .map_err(|_| Status::InternalServerError)?;
    Ok(Json(results))
}

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
                admin_pages_new_get,
                admin_pages_new_post,
                calendar_index,
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
