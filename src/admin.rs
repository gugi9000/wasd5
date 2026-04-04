use std::fs;
use std::path::{Path, PathBuf};

use bcrypt::{DEFAULT_COST, hash, verify};
use chrono::Utc;
use diesel::prelude::*;
use rocket::FromForm;
use rocket::form::Form;
use rocket::fs::TempFile;
use rocket::http::{Cookie, CookieJar, Status};
use rocket::response::Redirect;
use rocket::serde::{Deserialize, json::Json};
use rocket::{State, get, post};
use rocket_dyn_templates::{Template, context};
use serde::Serialize;

use crate::db::DbPool;
use crate::models;

const PAGES_DIR: &str = "pages";
const STATIC_FILES_DIR: &str = "static/files";
const STATIC_PICTURES_DIR: &str = "static/pictures";

#[derive(Serialize)]
struct AssetItem {
    name: String,
    url: String,
    markdown: String,
    rel_path: String,
}

#[derive(Serialize)]
struct PictureFolderItem {
    name: String,
    path: String,
    is_current: bool,
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

fn is_safe_folder_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn normalize_picture_folder(input: &str) -> Option<String> {
    let raw = input.trim().replace('\\', "/");
    if raw.is_empty() {
        return Some(String::new());
    }

    let mut parts = Vec::new();
    for part in raw.split('/') {
        let segment = part.trim();
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." || !is_safe_folder_segment(segment) {
            return None;
        }
        parts.push(segment.to_string());
    }

    Some(parts.join("/"))
}

fn picture_folder_fs_path(folder: &str) -> PathBuf {
    let mut path = PathBuf::from(STATIC_PICTURES_DIR);
    if !folder.is_empty() {
        for segment in folder.split('/') {
            path.push(segment);
        }
    }
    path
}

fn picture_folder_url(folder: &str) -> String {
    if folder.is_empty() {
        "/static/pictures".to_string()
    } else {
        format!("/static/pictures/{}", folder)
    }
}

fn admin_pictures_url(folder: &str, warning: Option<&str>) -> String {
    let mut query_parts = Vec::new();
    if !folder.is_empty() {
        query_parts.push(format!("folder={}", folder));
    }
    if let Some(w) = warning {
        query_parts.push(format!("warning={}", w));
    }
    if query_parts.is_empty() {
        "/admin/pictures".to_string()
    } else {
        format!("/admin/pictures?{}", query_parts.join("&"))
    }
}

fn redirect_admin_pictures(folder: &str, warning: Option<&str>) -> Redirect {
    Redirect::to(admin_pictures_url(folder, warning))
}

fn collect_picture_folders_recursive(base: &Path, current: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let Ok(relative) = path.strip_prefix(base) else {
            continue;
        };

        let rel = relative.to_string_lossy().replace('\\', "/");
        if !rel.is_empty() {
            out.push(rel.clone());
            collect_picture_folders_recursive(base, &path, out);
        }
    }
}

fn list_picture_folders() -> Vec<String> {
    let base = Path::new(STATIC_PICTURES_DIR);
    let mut folders = vec![String::new()];
    if fs::create_dir_all(base).is_err() {
        return folders;
    }
    collect_picture_folders_recursive(base, base, &mut folders);
    folders.sort_by_key(|f| f.to_ascii_lowercase());
    folders.dedup();
    folders
}

fn list_picture_assets_for_folder(folder: &str) -> Vec<AssetItem> {
    let mut items = Vec::new();
    let folder_path = picture_folder_fs_path(folder);
    let url_prefix = picture_folder_url(folder);

    let entries = match fs::read_dir(folder_path) {
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
        items.push(AssetItem {
            name: name.to_string(),
            url: url.clone(),
            markdown: format!("![]({})", url),
            rel_path: if folder.is_empty() {
                name.to_string()
            } else {
                format!("{}/{}", folder, name)
            },
        });
    }

    items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    items
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
            rel_path: name.to_string(),
        });
    }

    items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    items
}

#[derive(FromForm)]
pub(crate) struct LoginForm {
    username: String,
    password: String,
}

#[get("/admin/login")]
pub(crate) fn admin_login_get(jar: &CookieJar) -> Template {
    let pages = crate::read_pages();
    let csrf = crate::ensure_csrf(jar);
    Template::render("admin/login", context! { pages: pages, csrf: csrf })
}

#[post("/admin/login", data = "<form>")]
pub(crate) fn admin_login_post(
    form: Form<LoginForm>,
    jar: &CookieJar,
    pool: &State<DbPool>,
) -> Result<Redirect, Template> {
    use crate::schema::users::dsl::*;
    let f = form.into_inner();
    let mut conn = pool.get().map_err(|_| {
        let pages = crate::read_pages();
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
    let pages = crate::read_pages();
    let csrf = crate::ensure_csrf(jar);
    Err(Template::render(
        "admin/login",
        context! { error: "Invalid credentials", pages: pages, csrf: csrf },
    ))
}

#[get("/admin/logout")]
pub(crate) fn admin_logout(jar: &CookieJar) -> Redirect {
    jar.remove_private(Cookie::new("user_id", ""));
    jar.remove_private(Cookie::new("username", ""));
    jar.remove_private(Cookie::new("user_role", ""));
    jar.remove_private(Cookie::new("session_expires", ""));
    Redirect::to("/admin/login")
}

#[get("/admin")]
pub(crate) fn admin_index(jar: &CookieJar) -> Result<Template, Redirect> {
    if !crate::is_admin_cookie(jar) {
        return Err(Redirect::to("/admin/login"));
    }
    let pages = crate::read_pages();
    Ok(Template::render(
        "admin/index",
        context! { pages: pages },
    ))
}

#[get("/admin/files?<warning>")]
pub(crate) fn admin_files_get(
    jar: &CookieJar,
    warning: Option<&str>,
) -> Result<Template, Redirect> {
    if !crate::is_admin_cookie(jar) {
        return Err(Redirect::to("/admin/login"));
    }
    let pages = crate::read_pages();
    let csrf = crate::ensure_csrf(jar);
    let files = list_static_assets(STATIC_FILES_DIR, "/static/files", false);
    Ok(Template::render(
        "admin/files",
        context! { pages: pages, csrf: csrf, files: files, warning: warning },
    ))
}

#[get("/admin/pictures?<warning>&<folder>")]
pub(crate) fn admin_pictures_get(
    jar: &CookieJar,
    warning: Option<&str>,
    folder: Option<&str>,
) -> Result<Template, Redirect> {
    if !crate::is_admin_cookie(jar) {
        return Err(Redirect::to("/admin/login"));
    }

    let folder = match folder {
        Some(value) => match normalize_picture_folder(value) {
            Some(v) => v,
            None => return Ok(Template::render(
                "admin/pictures",
                context! {
                    pages: crate::read_pages(),
                    csrf: crate::ensure_csrf(jar),
                    pictures: Vec::<AssetItem>::new(),
                    picture_folders: Vec::<PictureFolderItem>::new(),
                    all_folders: vec![String::new()],
                    current_folder: String::new(),
                    current_folder_label: String::from("/"),
                    current_folder_static_path: String::from("/static/pictures"),
                    warning: Some("invalid_folder"),
                },
            )),
        },
        None => String::new(),
    };

    let pages = crate::read_pages();
    let csrf = crate::ensure_csrf(jar);

    let folder_path = picture_folder_fs_path(&folder);
    if fs::create_dir_all(&folder_path).is_err() {
        return Ok(Template::render(
            "admin/pictures",
            context! {
                pages: pages,
                csrf: csrf,
                pictures: Vec::<AssetItem>::new(),
                picture_folders: Vec::<PictureFolderItem>::new(),
                all_folders: vec![String::new()],
                current_folder: folder,
                current_folder_label: String::from("/"),
                current_folder_static_path: String::from("/static/pictures"),
                warning: Some("folder_missing"),
            },
        ));
    }

    let pictures = list_picture_assets_for_folder(&folder);
    let all_folders = list_picture_folders();
    let picture_folders = all_folders
        .iter()
        .map(|entry| PictureFolderItem {
            name: if entry.is_empty() {
                "/".to_string()
            } else {
                entry.rsplit('/').next().unwrap_or(entry).to_string()
            },
            path: entry.clone(),
            is_current: *entry == folder,
        })
        .collect::<Vec<_>>();

    let current_folder_label = if folder.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", folder)
    };
    let current_folder_static_path = picture_folder_url(&folder);

    Ok(Template::render(
        "admin/pictures",
        context! {
            pages: pages,
            csrf: csrf,
            pictures: pictures,
            picture_folders: picture_folders,
            all_folders: all_folders,
            current_folder: folder,
            current_folder_label: current_folder_label,
            current_folder_static_path: current_folder_static_path,
            warning: warning,
        },
    ))
}

#[get("/admin/calendar")]
pub(crate) fn admin_calendar_settings_get(
    jar: &CookieJar,
    pool: &State<DbPool>,
) -> Result<Template, Redirect> {
    if !crate::is_admin_cookie(jar) {
        return Err(Redirect::to("/admin/login"));
    }
    let pages = crate::read_pages();
    let csrf = crate::ensure_csrf(jar);
    use crate::schema::calendar_allowed_ips::dsl as aid;
    let calendar_allowed_ips_text = if let Ok(mut conn) = pool.get() {
        aid::calendar_allowed_ips
            .order(aid::ip_address.asc())
            .load::<models::CalendarAllowedIp>(&mut conn)
            .map(|rows| {
                rows.into_iter()
                    .map(|r| r.ip_address)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    } else {
        String::new()
    };
    Ok(Template::render(
        "admin/calendar_settings",
        context! { pages: pages, csrf: csrf, calendar_allowed_ips_text: calendar_allowed_ips_text },
    ))
}

#[derive(FromForm)]
pub(crate) struct CalendarAllowedIpsForm {
    allowed_ips: String,
    csrf: Option<String>,
}

#[post("/admin/calendar/allowed-ips", data = "<form>")]
pub(crate) fn admin_update_calendar_allowed_ips(
    jar: &CookieJar,
    pool: &State<DbPool>,
    form: Form<CalendarAllowedIpsForm>,
) -> Redirect {
    if !crate::is_admin_cookie(jar) {
        return Redirect::to("/admin/login");
    }

    let f = form.into_inner();
    if let Some(form_csrf) = f.csrf.as_ref() {
        if let Some(cookie_csrf) = jar.get_private("csrf") {
            if cookie_csrf.value() != form_csrf.as_str() {
                return Redirect::to("/admin/calendar");
            }
        } else {
            return Redirect::to("/admin/calendar");
        }
    } else {
        return Redirect::to("/admin/calendar");
    }

    let mut parsed: Vec<String> = Vec::new();
    for line in f.allowed_ips.lines() {
        let ip = line.trim();
        if ip.is_empty() {
            continue;
        }
        if ip.len() > 64 {
            continue;
        }
        if !ip
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '.' || c == ':' )
        {
            continue;
        }
        if !parsed.iter().any(|v| v == ip) {
            parsed.push(ip.to_string());
        }
    }

    use crate::schema::calendar_allowed_ips::dsl as aid;
    if let Ok(mut conn) = pool.get() {
        let _ = diesel::delete(aid::calendar_allowed_ips).execute(&mut conn);
        for ip in parsed {
            let row = models::NewCalendarAllowedIp {
                ip_address: &ip,
                created_at: Utc::now().timestamp(),
            };
            let _ = diesel::insert_into(aid::calendar_allowed_ips)
                .values(&row)
                .execute(&mut conn);
        }
    }

    Redirect::to("/admin/calendar")
}

#[derive(FromForm)]
pub(crate) struct UploadForm<'r> {
    upload: TempFile<'r>,
    target_folder: Option<String>,
    csrf: Option<String>,
}

#[post("/admin/upload/file", data = "<form>")]
pub(crate) async fn admin_upload_file(jar: &CookieJar<'_>, form: Form<UploadForm<'_>>) -> Redirect {
    if !crate::is_admin_cookie(jar) {
        return Redirect::to("/admin/login");
    }

    let mut f = form.into_inner();
    if let Some(form_csrf) = f.csrf.as_ref() {
        if let Some(cookie_csrf) = jar.get_private("csrf") {
            if cookie_csrf.value() != form_csrf.as_str() {
                return Redirect::to("/admin/files");
            }
        } else {
            return Redirect::to("/admin/files");
        }
    } else {
        return Redirect::to("/admin/files");
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
        return Redirect::to("/admin/files");
    }
    let target = unique_upload_path(Path::new(STATIC_FILES_DIR), &filename);
    if f.upload.persist_to(&target).await.is_err() {
        return Redirect::to("/admin/files");
    }

    if sanitized_changed {
        Redirect::to("/admin/files?warning=filename_sanitized")
    } else {
        Redirect::to("/admin/files")
    }
}

#[post("/admin/upload/picture", data = "<form>")]
pub(crate) async fn admin_upload_picture(jar: &CookieJar<'_>, form: Form<UploadForm<'_>>) -> Redirect {
    if !crate::is_admin_cookie(jar) {
        return Redirect::to("/admin/login");
    }

    let mut f = form.into_inner();
    if let Some(form_csrf) = f.csrf.as_ref() {
        if let Some(cookie_csrf) = jar.get_private("csrf") {
            if cookie_csrf.value() != form_csrf.as_str() {
                return redirect_admin_pictures("", Some("invalid_csrf"));
            }
        } else {
            return redirect_admin_pictures("", Some("invalid_csrf"));
        }
    } else {
        return redirect_admin_pictures("", Some("invalid_csrf"));
    }

    let folder = match f.target_folder.as_deref() {
        Some(value) if !value.trim().is_empty() => match normalize_picture_folder(value) {
            Some(v) => v,
            None => return redirect_admin_pictures("", Some("invalid_folder")),
        },
        _ => String::new(),
    };

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
        return redirect_admin_pictures(&folder, Some("invalid_picture_type"));
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

    let target_dir = picture_folder_fs_path(&folder);
    if fs::create_dir_all(&target_dir).is_err() {
        return redirect_admin_pictures(&folder, Some("folder_create_failed"));
    }
    let target = unique_upload_path(&target_dir, &filename);
    if f.upload.persist_to(&target).await.is_err() {
        return redirect_admin_pictures(&folder, Some("upload_failed"));
    }

    if sanitized_changed {
        redirect_admin_pictures(&folder, Some("filename_sanitized"))
    } else {
        redirect_admin_pictures(&folder, None)
    }
}

#[derive(FromForm)]
pub(crate) struct CreatePictureFolderForm {
    parent_folder: Option<String>,
    folder_name: String,
    csrf: Option<String>,
}

#[post("/admin/pictures/folder/create", data = "<form>")]
pub(crate) fn admin_create_picture_folder(
    jar: &CookieJar<'_>,
    form: Form<CreatePictureFolderForm>,
) -> Redirect {
    if !crate::is_admin_cookie(jar) {
        return Redirect::to("/admin/login");
    }

    let f = form.into_inner();
    if let Some(form_csrf) = f.csrf.as_ref() {
        if let Some(cookie_csrf) = jar.get_private("csrf") {
            if cookie_csrf.value() != form_csrf.as_str() {
                return redirect_admin_pictures("", Some("invalid_csrf"));
            }
        } else {
            return redirect_admin_pictures("", Some("invalid_csrf"));
        }
    } else {
        return redirect_admin_pictures("", Some("invalid_csrf"));
    }

    let parent = match f.parent_folder.as_deref() {
        Some(value) if !value.trim().is_empty() => match normalize_picture_folder(value) {
            Some(v) => v,
            None => return redirect_admin_pictures("", Some("invalid_folder")),
        },
        _ => String::new(),
    };

    let folder_name_original = f.folder_name.trim();
    let folder_name = sanitize_upload_filename(folder_name_original);
    if folder_name.is_empty() {
        return redirect_admin_pictures(&parent, Some("invalid_folder_name"));
    }
    if !is_safe_folder_segment(&folder_name) {
        return redirect_admin_pictures(&parent, Some("invalid_folder_name"));
    }

    let new_folder_rel = if parent.is_empty() {
        folder_name.clone()
    } else {
        format!("{}/{}", parent, folder_name)
    };
    let new_folder_path = picture_folder_fs_path(&new_folder_rel);
    if new_folder_path.exists() {
        return redirect_admin_pictures(&new_folder_rel, Some("folder_exists"));
    }

    if fs::create_dir_all(&new_folder_path).is_err() {
        return redirect_admin_pictures(&parent, Some("folder_create_failed"));
    }

    if folder_name != folder_name_original {
        redirect_admin_pictures(&new_folder_rel, Some("folder_name_sanitized"))
    } else {
        redirect_admin_pictures(&new_folder_rel, Some("folder_created"))
    }
}

#[derive(FromForm)]
pub(crate) struct PictureRenameMoveForm {
    source_path: String,
    target_folder: Option<String>,
    new_name: Option<String>,
    csrf: Option<String>,
}

#[post("/admin/pictures/rename-move", data = "<form>")]
pub(crate) fn admin_rename_move_picture(jar: &CookieJar<'_>, form: Form<PictureRenameMoveForm>) -> Redirect {
    if !crate::is_admin_cookie(jar) {
        return Redirect::to("/admin/login");
    }

    let f = form.into_inner();
    if let Some(form_csrf) = f.csrf.as_ref() {
        if let Some(cookie_csrf) = jar.get_private("csrf") {
            if cookie_csrf.value() != form_csrf.as_str() {
                return redirect_admin_pictures("", Some("invalid_csrf"));
            }
        } else {
            return redirect_admin_pictures("", Some("invalid_csrf"));
        }
    } else {
        return redirect_admin_pictures("", Some("invalid_csrf"));
    }

    let source_rel = match normalize_picture_folder(&f.source_path) {
        Some(v) if !v.is_empty() => v,
        _ => return redirect_admin_pictures("", Some("invalid_picture_operation")),
    };
    let source_file_name = source_rel.rsplit('/').next().unwrap_or("image.bin");
    let source_folder = source_rel
        .rsplit_once('/')
        .map(|(folder, _)| folder.to_string())
        .unwrap_or_default();

    let target_folder = match f.target_folder.as_deref() {
        Some(value) if !value.trim().is_empty() => match normalize_picture_folder(value) {
            Some(v) => v,
            None => return redirect_admin_pictures(&source_folder, Some("invalid_folder")),
        },
        _ => source_folder.clone(),
    };

    let requested_name = f.new_name.unwrap_or_default();
    let sanitized_name = if requested_name.trim().is_empty() {
        source_file_name.to_string()
    } else {
        sanitize_upload_filename(requested_name.trim())
    };
    if sanitized_name.is_empty() {
        return redirect_admin_pictures(&source_folder, Some("invalid_picture_name"));
    }
    if !is_allowed_picture_filename(&sanitized_name) {
        return redirect_admin_pictures(&source_folder, Some("invalid_picture_name"));
    }

    let source_path = picture_folder_fs_path(&source_rel);
    if !source_path.is_file() {
        return redirect_admin_pictures(&source_folder, Some("picture_not_found"));
    }

    let target_dir = picture_folder_fs_path(&target_folder);
    if fs::create_dir_all(&target_dir).is_err() {
        return redirect_admin_pictures(&source_folder, Some("folder_create_failed"));
    }

    let destination = unique_upload_path(&target_dir, &sanitized_name);
    if fs::rename(&source_path, &destination).is_err() {
        return redirect_admin_pictures(&source_folder, Some("rename_move_failed"));
    }

    if sanitized_name != requested_name.trim() {
        redirect_admin_pictures(&target_folder, Some("filename_sanitized"))
    } else {
        redirect_admin_pictures(&target_folder, Some("picture_renamed_moved"))
    }
}

#[get("/admin/users")]
pub(crate) fn admin_users(jar: &CookieJar, pool: &State<DbPool>) -> Result<Template, Redirect> {
    if !crate::is_admin_cookie(jar) {
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
    let pages = crate::read_pages();
    Ok(Template::render(
        "admin/users",
        context! { users: results, pages: pages },
    ))
}

#[get("/admin/users/new")]
pub(crate) fn admin_users_new(jar: &CookieJar) -> Result<Template, Redirect> {
    if !crate::is_admin_cookie(jar) {
        return Err(Redirect::to("/admin/login"));
    }
    let pages = crate::read_pages();
    let csrf = crate::ensure_csrf(jar);
    Ok(Template::render(
        "admin/new_user",
        context! { pages: pages, csrf: csrf },
    ))
}

#[derive(FromForm)]
pub(crate) struct NewUserForm {
    username: String,
    password: String,
    role: Option<String>,
    csrf: Option<String>,
}

#[post("/admin/users", data = "<form>")]
pub(crate) fn admin_users_create(jar: &CookieJar, pool: &State<DbPool>, form: Form<NewUserForm>) -> Redirect {
    if !crate::is_admin_cookie(jar) {
        return Redirect::to("/admin/login");
    }
    use crate::schema::users::dsl::users;
    let f = form.into_inner();
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
pub(crate) struct LandingForm {
    content: String,
    csrf: Option<String>,
}

#[get("/admin/landing")]
pub(crate) fn admin_landing_get(jar: &CookieJar) -> Result<Template, Redirect> {
    if !crate::is_admin_cookie(jar) {
        return Err(Redirect::to("/admin/login"));
    }
    let pages = crate::read_pages();
    let mut landing_md = String::from("# Welcome\n\nWelcome to the site.");
    let mut path = PathBuf::from(PAGES_DIR);
    path.push("landing.md");
    if let Ok(s) = fs::read_to_string(&path) {
        landing_md = s;
    }
    let csrf = crate::ensure_csrf(jar);
    Ok(Template::render(
        "admin/landing_edit",
        context! { pages: pages, content: landing_md, csrf: csrf },
    ))
}

#[post("/admin/landing", data = "<form>")]
pub(crate) fn admin_landing_post(jar: &CookieJar, form: Form<LandingForm>) -> Redirect {
    if !crate::is_admin_cookie(jar) {
        return Redirect::to("/admin/login");
    }
    let f = form.into_inner();
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

#[derive(FromForm)]
pub(crate) struct NewPageForm {
    slug: String,
    content: String,
    csrf: Option<String>,
}

#[get("/admin/pages/new")]
pub(crate) fn admin_pages_new_get(jar: &CookieJar) -> Result<Template, Redirect> {
    if !crate::is_admin_cookie(jar) {
        return Err(Redirect::to("/admin/login"));
    }
    let pages = crate::read_pages();
    let csrf = crate::ensure_csrf(jar);
    Ok(Template::render(
        "admin/new_page",
        context! { pages: pages, csrf: csrf },
    ))
}

#[post("/admin/pages/new", data = "<form>")]
pub(crate) fn admin_pages_new_post(jar: &CookieJar, form: Form<NewPageForm>) -> Redirect {
    if !crate::is_admin_cookie(jar) {
        return Redirect::to("/admin/login");
    }
    let f = form.into_inner();
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

    if fs::write(&page_path, f.content.as_bytes()).is_err() {
        return Redirect::to("/admin/pages/new");
    }

    Redirect::to(format!("/admin/pages/edit/{}", slug))
}

#[derive(FromForm)]
pub(crate) struct EditPageForm {
    content: String,
    csrf: Option<String>,
}

#[get("/admin/pages/edit/<path..>")]
pub(crate) fn admin_edit_page_get(path: std::path::PathBuf, jar: &CookieJar) -> Result<Template, Redirect> {
    if !crate::is_admin_cookie(jar) {
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
    let pages = crate::read_pages();
    let csrf = crate::ensure_csrf(jar);
    Ok(Template::render(
        "admin/edit_page",
        context! { pages: pages, slug: slug, content: content, csrf: csrf },
    ))
}

#[post("/admin/pages/edit/<path..>", data = "<form>")]
pub(crate) fn admin_edit_page_post(
    path: std::path::PathBuf,
    jar: &CookieJar,
    form: Form<EditPageForm>,
) -> Redirect {
    if !crate::is_admin_cookie(jar) {
        return Redirect::to("/admin/login");
    }
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
    if let Some(parent) = page_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    page_path.set_extension("md");

    if fs::write(&page_path, f.content.as_bytes()).is_err() {
        return Redirect::to(format!("/admin/pages/edit/{}", slug));
    }

    Redirect::to(format!("/page/{}", slug))
}

#[derive(Deserialize)]
pub(crate) struct CreateUser {
    username: String,
    password: String,
    role: Option<String>,
}

#[post("/api/admin/users", data = "<payload>")]
pub(crate) fn create_user(
    jar: &CookieJar,
    pool: &State<DbPool>,
    payload: Json<CreateUser>,
) -> Result<String, Status> {
    if !crate::is_admin_cookie(jar) {
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
pub(crate) fn list_users(jar: &CookieJar, pool: &State<DbPool>) -> Result<Json<Vec<models::User>>, Status> {
    if !crate::is_admin_cookie(jar) {
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