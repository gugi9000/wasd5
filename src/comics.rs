use std::path::{Path, PathBuf};

use diesel::prelude::*;
use rocket::form::Form;
use rocket::fs::{NamedFile, TempFile};
use rocket::http::{CookieJar, Status};
use rocket::request::{FromRequest, Outcome, Request};
use rocket::response::{self, Redirect, Responder};
use rocket::{FromForm, State, get, post};
use rocket_dyn_templates::{Template, context};
use serde::Serialize;

use crate::db::DbPool;
use crate::schema::{comic_albums as albums, comic_strips as strips};

const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
type ComicError = (Status, String);
type ComicResult<T> = Result<Private<T>, ComicError>;

pub struct ComicStorage(pub PathBuf);

impl ComicStorage {
    pub fn initialize(root: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        std::fs::create_dir_all(&root)?;
        let root = root.canonicalize()?;
        if root.starts_with(Path::new("static").canonicalize()?) {
            return Err("COMICS_DIR must be outside the public static directory".into());
        }
        Ok(Self(root))
    }
}

pub fn routes() -> Vec<rocket::Route> {
    rocket::routes![
        index,
        new_album,
        create_album,
        album,
        edit_album,
        update_album,
        new_strip,
        upload_strip,
        edit_strip,
        update_strip,
        strip_image,
        banner_image,
        upload_banner,
        remove_banner,
    ]
}

pub struct Private<T>(T);

impl<'r, T: Responder<'r, 'static>> Responder<'r, 'static> for Private<T> {
    fn respond_to(self, request: &'r Request<'_>) -> response::Result<'static> {
        let mut response = self.0.respond_to(request)?;
        response.set_raw_header("Cache-Control", "private, no-store");
        response.set_raw_header("Vary", "Cookie");
        Ok(response)
    }
}

#[derive(Serialize)]
pub struct ComicUser {
    id: i32,
    username: String,
    admin: bool,
}

pub struct Viewer(Option<ComicUser>);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Viewer {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let jar = request.cookies();
        let expiry = jar
            .get_private("session_expires")
            .and_then(|c| c.value().parse::<i64>().ok());
        let id = jar
            .get_private("user_id")
            .and_then(|c| c.value().parse::<i32>().ok());
        let Some(id) = id.filter(|_| expiry.is_some_and(|ts| ts > chrono::Utc::now().timestamp()))
        else {
            return Outcome::Success(Viewer(None));
        };
        let Some(pool) = request.rocket().state::<DbPool>() else {
            eprintln!("Comics database pool unavailable");
            return Outcome::Error((Status::InternalServerError, ()));
        };
        let user = (|| -> Result<_, Box<dyn std::error::Error>> {
            let mut conn = pool.get()?;
            Ok(crate::schema::users::table
                .find(id)
                .first::<crate::models::User>(&mut conn)
                .optional()?)
        })();
        match user {
            Ok(user) => Outcome::Success(Viewer(user.map(|u| ComicUser {
                id: u.id,
                username: u.username,
                admin: u.role == "admin",
            }))),
            Err(error) => {
                eprintln!("Failed to authenticate comics viewer: {error}");
                Outcome::Error((Status::InternalServerError, ()))
            }
        }
    }
}

impl Viewer {
    fn user(&self) -> Result<&ComicUser, ComicError> {
        self.0
            .as_ref()
            .ok_or((Status::Unauthorized, "Sign in to manage comics".into()))
    }

    fn can_manage(&self, album: &Album) -> bool {
        self.0
            .as_ref()
            .is_some_and(|user| user.admin || user.id == album.owner_id)
    }

    fn require_manager(&self, album: &Album) -> Result<(), ComicError> {
        if self.can_manage(album) {
            Ok(())
        } else {
            Err((
                Status::Forbidden,
                "Only the album owner or an admin can change this album".into(),
            ))
        }
    }
}

#[derive(Queryable, Identifiable, Serialize)]
#[diesel(table_name = albums)]
struct Album {
    id: i32,
    owner_id: i32,
    title: String,
    subtext: String,
    hidden: bool,
    created_at: i64,
    updated_at: i64,
    banner_filename: Option<String>,
}

#[derive(Queryable, Identifiable, Serialize)]
#[diesel(table_name = strips)]
struct Strip {
    id: i32,
    album_id: i32,
    title: String,
    subtext: String,
    image_filename: String,
    hidden: bool,
    display_order: i32,
    created_at: i64,
    updated_at: i64,
}

fn internal(error: impl std::fmt::Display) -> ComicError {
    eprintln!("Comics operation failed: {error}");
    (
        Status::InternalServerError,
        "The comics operation could not be completed. Please try again.".into(),
    )
}

fn not_found() -> ComicError {
    (Status::NotFound, "Comic not found".into())
}

fn get_album(conn: &mut SqliteConnection, id: i32) -> Result<Album, ComicError> {
    albums::table
        .find(id)
        .first(conn)
        .optional()
        .map_err(internal)?
        .ok_or_else(not_found)
}

fn get_strip(conn: &mut SqliteConnection, album_id: i32, id: i32) -> Result<Strip, ComicError> {
    strips::table
        .find(id)
        .filter(strips::album_id.eq(album_id))
        .first(conn)
        .optional()
        .map_err(internal)?
        .ok_or_else(not_found)
}

fn visible(viewer: &Viewer, album: &Album, strip: Option<&Strip>) -> Result<(), ComicError> {
    if viewer.can_manage(album) || (!album.hidden && !strip.is_some_and(|s| s.hidden)) {
        Ok(())
    } else {
        Err(not_found())
    }
}

fn csrf(jar: &CookieJar<'_>, token: &str) -> Result<(), ComicError> {
    if jar.get_private("csrf").is_some_and(|c| c.value() == token) {
        Ok(())
    } else {
        Err((
            Status::Forbidden,
            "Invalid form token. Reload the form and try again.".into(),
        ))
    }
}

fn validate_text(title: &str, subtext: &str) -> Result<(), ComicError> {
    if title.trim().is_empty()
        || title.trim().chars().count() > 200
        || subtext.chars().count() > 10_000
    {
        Err((
            Status::BadRequest,
            "A title of 1-200 characters is required; subtext must not exceed 10,000 characters."
                .into(),
        ))
    } else {
        Ok(())
    }
}

#[derive(FromForm)]
pub struct AlbumForm {
    title: String,
    subtext: String,
    hidden: bool,
    csrf: String,
}

#[get("/comics")]
fn index(
    viewer: Viewer,
    pool: &State<DbPool>,
    navigation: crate::PageNavigation,
) -> ComicResult<Template> {
    let mut conn = pool.get().map_err(internal)?;
    let mut query = albums::table.into_boxed();
    match &viewer.0 {
        Some(user) if user.admin => {}
        Some(user) => {
            query = query.filter(albums::hidden.eq(false).or(albums::owner_id.eq(user.id)))
        }
        None => query = query.filter(albums::hidden.eq(false)),
    }
    let items = query
        .order((albums::updated_at.desc(), albums::id.desc()))
        .load::<Album>(&mut conn)
        .map_err(internal)?;
    Ok(Private(Template::render(
        "comics/index",
        context! { pages: navigation.0, albums: items, user: viewer.0 },
    )))
}

#[get("/comics/new")]
fn new_album(
    viewer: Viewer,
    jar: &CookieJar<'_>,
    navigation: crate::PageNavigation,
) -> ComicResult<Template> {
    viewer.user()?;
    Ok(Private(Template::render(
        "comics/album_form",
        context! {
            pages: navigation.0, album: Option::<Album>::None, csrf: crate::ensure_csrf(jar),
        },
    )))
}

#[post("/comics/new", data = "<form>")]
fn create_album(
    viewer: Viewer,
    jar: &CookieJar<'_>,
    pool: &State<DbPool>,
    form: Form<AlbumForm>,
) -> ComicResult<Redirect> {
    let user = viewer.user()?;
    let form = form.into_inner();
    csrf(jar, &form.csrf)?;
    validate_text(&form.title, &form.subtext)?;
    let mut conn = pool.get().map_err(internal)?;
    let now = chrono::Utc::now().timestamp();
    let id = conn
        .transaction::<_, diesel::result::Error, _>(|conn| {
            diesel::insert_into(albums::table)
                .values((
                    albums::owner_id.eq(user.id),
                    albums::title.eq(form.title.trim()),
                    albums::subtext.eq(&form.subtext),
                    albums::hidden.eq(form.hidden),
                    albums::created_at.eq(now),
                    albums::updated_at.eq(now),
                ))
                .execute(conn)?;
            albums::table
                .select(albums::id)
                .order(albums::id.desc())
                .first::<i32>(conn)
        })
        .map_err(internal)?;
    Ok(Private(Redirect::to(format!("/comics/{id}"))))
}

#[get("/comics/<id>")]
fn album(
    id: i32,
    viewer: Viewer,
    pool: &State<DbPool>,
    navigation: crate::PageNavigation,
) -> ComicResult<Template> {
    let mut conn = pool.get().map_err(internal)?;
    let album = get_album(&mut conn, id)?;
    visible(&viewer, &album, None)?;
    let manage = viewer.can_manage(&album);
    let mut query = strips::table.filter(strips::album_id.eq(id)).into_boxed();
    if !manage {
        query = query.filter(strips::hidden.eq(false));
    }
    let items = query
        .order((strips::display_order.asc(), strips::id.asc()))
        .load::<Strip>(&mut conn)
        .map_err(internal)?;
    let owner = crate::schema::users::table
        .find(album.owner_id)
        .select(crate::schema::users::username)
        .first::<String>(&mut conn)
        .map_err(internal)?;
    Ok(Private(Template::render(
        "comics/album",
        context! {
            pages: navigation.0, album: album, strips: items, can_manage: manage, owner: owner,
        },
    )))
}

#[get("/comics/<id>/edit")]
fn edit_album(
    id: i32,
    viewer: Viewer,
    jar: &CookieJar<'_>,
    pool: &State<DbPool>,
    navigation: crate::PageNavigation,
) -> ComicResult<Template> {
    let mut conn = pool.get().map_err(internal)?;
    let album = get_album(&mut conn, id)?;
    visible(&viewer, &album, None)?;
    viewer.require_manager(&album)?;
    Ok(Private(Template::render(
        "comics/album_form",
        context! {
            pages: navigation.0, album: album, csrf: crate::ensure_csrf(jar),
        },
    )))
}

#[post("/comics/<id>/edit", data = "<form>")]
fn update_album(
    id: i32,
    viewer: Viewer,
    jar: &CookieJar<'_>,
    pool: &State<DbPool>,
    form: Form<AlbumForm>,
) -> ComicResult<Redirect> {
    viewer.user()?;
    let form = form.into_inner();
    csrf(jar, &form.csrf)?;
    validate_text(&form.title, &form.subtext)?;
    let mut conn = pool.get().map_err(internal)?;
    let album = get_album(&mut conn, id)?;
    visible(&viewer, &album, None)?;
    viewer.require_manager(&album)?;
    diesel::update(albums::table.find(id))
        .set((
            albums::title.eq(form.title.trim()),
            albums::subtext.eq(&form.subtext),
            albums::hidden.eq(form.hidden),
            albums::updated_at.eq(chrono::Utc::now().timestamp()),
        ))
        .execute(&mut conn)
        .map_err(internal)?;
    Ok(Private(Redirect::to(format!("/comics/{id}"))))
}

#[derive(FromForm)]
pub struct StripForm {
    title: String,
    subtext: String,
    hidden: bool,
    display_order: i32,
    csrf: String,
}

#[derive(FromForm)]
pub struct UploadForm<'r> {
    title: String,
    subtext: String,
    hidden: bool,
    display_order: i32,
    csrf: String,
    image: TempFile<'r>,
}

fn validate_strip(title: &str, subtext: &str, order: i32) -> Result<(), ComicError> {
    validate_text(title, subtext)?;
    if order < 0 {
        return Err((
            Status::BadRequest,
            "Display order must be zero or greater".into(),
        ));
    }
    Ok(())
}

#[get("/comics/<id>/strips/new")]
fn new_strip(
    id: i32,
    viewer: Viewer,
    jar: &CookieJar<'_>,
    pool: &State<DbPool>,
    navigation: crate::PageNavigation,
) -> ComicResult<Template> {
    let mut conn = pool.get().map_err(internal)?;
    let album = get_album(&mut conn, id)?;
    visible(&viewer, &album, None)?;
    viewer.require_manager(&album)?;
    let max = strips::table
        .filter(strips::album_id.eq(id))
        .select(diesel::dsl::max(strips::display_order))
        .first::<Option<i32>>(&mut conn)
        .map_err(internal)?;
    let next = match max {
        Some(value) => value.checked_add(1).ok_or((
            Status::BadRequest,
            "No more display positions available".into(),
        ))?,
        None => 0,
    };
    Ok(Private(Template::render(
        "comics/strip_form",
        context! {
            pages: navigation.0, album: album, strip: Option::<Strip>::None,
            next_order: next, csrf: crate::ensure_csrf(jar),
        },
    )))
}

fn validate_image(path: &Path) -> Result<&'static str, ComicError> {
    let mut reader = image::ImageReader::open(path)
        .map_err(internal)?
        .with_guessed_format()
        .map_err(internal)?;
    let extension = match reader.format() {
        Some(image::ImageFormat::Png) => "png",
        Some(image::ImageFormat::Jpeg) => "jpg",
        Some(image::ImageFormat::Gif) => "gif",
        Some(image::ImageFormat::WebP) => "webp",
        _ => {
            return Err((
                Status::BadRequest,
                "Upload a PNG, JPEG, GIF, or WebP image (SVG is not supported)".into(),
            ));
        }
    };
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().map_err(|error| {
        eprintln!("Comics image rejected: {error}");
        (
            Status::BadRequest,
            "The image is invalid or exceeds the supported dimensions/memory limit".into(),
        )
    })?;
    Ok(extension)
}

#[post("/comics/<id>/strips/new", data = "<form>")]
async fn upload_strip(
    id: i32,
    viewer: Viewer,
    jar: &CookieJar<'_>,
    pool: &State<DbPool>,
    storage: &State<ComicStorage>,
    form: Form<UploadForm<'_>>,
) -> ComicResult<Redirect> {
    viewer.user()?;
    let mut form = form.into_inner();
    csrf(jar, &form.csrf)?;
    validate_strip(&form.title, &form.subtext, form.display_order)?;
    {
        let mut conn = pool.get().map_err(internal)?;
        let album = get_album(&mut conn, id)?;
        visible(&viewer, &album, None)?;
        viewer.require_manager(&album)?;
    }
    let (filename, destination) = store_image(&mut form.image, storage, id).await?;
    let save = (|| {
        let mut conn = pool.get().map_err(internal)?;
        let now = chrono::Utc::now().timestamp();
        conn.transaction::<_, diesel::result::Error, _>(|conn| {
            diesel::insert_into(strips::table)
                .values((
                    strips::album_id.eq(id),
                    strips::title.eq(form.title.trim()),
                    strips::subtext.eq(&form.subtext),
                    strips::image_filename.eq(&filename),
                    strips::hidden.eq(form.hidden),
                    strips::display_order.eq(form.display_order),
                    strips::created_at.eq(now),
                    strips::updated_at.eq(now),
                ))
                .execute(conn)?;
            diesel::update(albums::table.find(id))
                .set(albums::updated_at.eq(now))
                .execute(conn)?;
            Ok(())
        })
        .map_err(internal)
    })();
    if let Err(error) = save {
        cleanup_image(&destination).await;
        return Err(error);
    }
    Ok(Private(Redirect::to(format!("/comics/{id}"))))
}

async fn cleanup_image(path: &Path) {
    if let Err(error) = delete_image(path).await {
        eprintln!("Failed to clean up comic image {}: {error}", path.display());
    }
}

async fn delete_image(path: &Path) -> std::io::Result<()> {
    match rocket::tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

async fn store_image(
    image: &mut TempFile<'_>,
    storage: &ComicStorage,
    album_id: i32,
) -> Result<(String, PathBuf), ComicError> {
    if image.len() == 0 || image.len() > MAX_IMAGE_BYTES {
        return Err((
            Status::PayloadTooLarge,
            "Images must be nonempty and no larger than 20 MiB".into(),
        ));
    }
    let source = image
        .path()
        .ok_or_else(|| internal("Upload temporary file missing"))?
        .to_path_buf();
    let extension = rocket::tokio::task::spawn_blocking(move || validate_image(&source))
        .await
        .map_err(internal)??;
    let folder = storage.0.join(album_id.to_string());
    rocket::tokio::fs::create_dir_all(&folder)
        .await
        .map_err(internal)?;
    let filename = format!("{}.{}", uuid::Uuid::new_v4(), extension);
    let destination = folder.join(&filename);
    if let Err(error) = image.copy_to(&destination).await {
        cleanup_image(&destination).await;
        return Err(internal(error));
    }
    Ok((filename, destination))
}

#[derive(FromForm)]
pub struct BannerForm<'r> {
    csrf: String,
    image: TempFile<'r>,
}

#[derive(FromForm)]
pub struct RemoveBannerForm {
    csrf: String,
}

fn change_banner(
    conn: &mut SqliteConnection,
    id: i32,
    viewer: &Viewer,
    filename: Option<&str>,
) -> Result<Option<String>, ComicError> {
    conn.transaction::<_, wasd5::pages::Error, _>(|conn| {
        let album = albums::table.find(id).first::<Album>(conn)?;
        if !viewer.can_manage(&album) {
            return Err("Album permission changed before banner save".into());
        }
        diesel::update(albums::table.find(id))
            .set((
                albums::banner_filename.eq(filename),
                albums::updated_at.eq(chrono::Utc::now().timestamp()),
            ))
            .execute(conn)?;
        Ok(album.banner_filename)
    })
    .map_err(internal)
}

async fn delete_old_banner(
    storage: &ComicStorage,
    id: i32,
    filename: Option<String>,
) -> Result<(), ComicError> {
    if let Some(filename) = filename {
        let path = image_path(storage, id, &filename)?;
        delete_image(&path).await.map_err(|error| {
            eprintln!("Banner metadata saved but old image cleanup failed: {error}");
            (
                Status::InternalServerError,
                "Banner saved, but old image cleanup failed. Contact an administrator.".into(),
            )
        })?;
    }
    Ok(())
}

#[post("/comics/<id>/banner", data = "<form>")]
async fn upload_banner(
    id: i32,
    viewer: Viewer,
    jar: &CookieJar<'_>,
    pool: &State<DbPool>,
    storage: &State<ComicStorage>,
    form: Form<BannerForm<'_>>,
) -> ComicResult<Redirect> {
    viewer.user()?;
    let mut form = form.into_inner();
    csrf(jar, &form.csrf)?;
    {
        let mut conn = pool.get().map_err(internal)?;
        let album = get_album(&mut conn, id)?;
        visible(&viewer, &album, None)?;
        viewer.require_manager(&album)?;
    }
    let (filename, destination) = store_image(&mut form.image, storage, id).await?;
    let save = (|| {
        let mut conn = pool.get().map_err(internal)?;
        change_banner(&mut conn, id, &viewer, Some(&filename))
    })();
    let old = match save {
        Ok(old) => old,
        Err(error) => {
            cleanup_image(&destination).await;
            return Err(error);
        }
    };
    delete_old_banner(storage, id, old).await?;
    Ok(Private(Redirect::to(format!("/comics/{id}/edit"))))
}

#[post("/comics/<id>/banner/remove", data = "<form>")]
async fn remove_banner(
    id: i32,
    viewer: Viewer,
    jar: &CookieJar<'_>,
    pool: &State<DbPool>,
    storage: &State<ComicStorage>,
    form: Form<RemoveBannerForm>,
) -> ComicResult<Redirect> {
    viewer.user()?;
    csrf(jar, &form.csrf)?;
    let old = {
        let mut conn = pool.get().map_err(internal)?;
        let album = get_album(&mut conn, id)?;
        visible(&viewer, &album, None)?;
        viewer.require_manager(&album)?;
        change_banner(&mut conn, id, &viewer, None)?
    };
    delete_old_banner(storage, id, old).await?;
    Ok(Private(Redirect::to(format!("/comics/{id}/edit"))))
}

#[get("/comics/<id>/banner")]
async fn banner_image(
    id: i32,
    viewer: Viewer,
    pool: &State<DbPool>,
    storage: &State<ComicStorage>,
) -> ComicResult<NamedFile> {
    let path = {
        let mut conn = pool.get().map_err(internal)?;
        let album = get_album(&mut conn, id)?;
        visible(&viewer, &album, None)?;
        image_path(
            storage,
            id,
            album.banner_filename.as_deref().ok_or_else(not_found)?,
        )?
    };
    Ok(Private(NamedFile::open(path).await.map_err(internal)?))
}

#[get("/comics/<album_id>/strips/<id>/edit")]
fn edit_strip(
    album_id: i32,
    id: i32,
    viewer: Viewer,
    jar: &CookieJar<'_>,
    pool: &State<DbPool>,
    navigation: crate::PageNavigation,
) -> ComicResult<Template> {
    let mut conn = pool.get().map_err(internal)?;
    let album = get_album(&mut conn, album_id)?;
    let strip = get_strip(&mut conn, album_id, id)?;
    visible(&viewer, &album, Some(&strip))?;
    viewer.require_manager(&album)?;
    Ok(Private(Template::render(
        "comics/strip_form",
        context! {
            pages: navigation.0, album: album, strip: strip, csrf: crate::ensure_csrf(jar),
        },
    )))
}

#[post("/comics/<album_id>/strips/<id>/edit", data = "<form>")]
fn update_strip(
    album_id: i32,
    id: i32,
    viewer: Viewer,
    jar: &CookieJar<'_>,
    pool: &State<DbPool>,
    form: Form<StripForm>,
) -> ComicResult<Redirect> {
    viewer.user()?;
    let form = form.into_inner();
    csrf(jar, &form.csrf)?;
    validate_strip(&form.title, &form.subtext, form.display_order)?;
    let mut conn = pool.get().map_err(internal)?;
    let album = get_album(&mut conn, album_id)?;
    let strip = get_strip(&mut conn, album_id, id)?;
    visible(&viewer, &album, Some(&strip))?;
    viewer.require_manager(&album)?;
    let now = chrono::Utc::now().timestamp();
    conn.transaction::<_, diesel::result::Error, _>(|conn| {
        diesel::update(strips::table.find(id))
            .set((
                strips::title.eq(form.title.trim()),
                strips::subtext.eq(&form.subtext),
                strips::hidden.eq(form.hidden),
                strips::display_order.eq(form.display_order),
                strips::updated_at.eq(now),
            ))
            .execute(conn)?;
        diesel::update(albums::table.find(album_id))
            .set(albums::updated_at.eq(now))
            .execute(conn)?;
        Ok(())
    })
    .map_err(internal)?;
    Ok(Private(Redirect::to(format!(
        "/comics/{album_id}#strip-{id}"
    ))))
}

fn image_path(
    storage: &ComicStorage,
    album_id: i32,
    filename: &str,
) -> Result<PathBuf, ComicError> {
    let Some((id, extension)) = filename.rsplit_once('.') else {
        return Err(internal("Invalid stored comics image name"));
    };
    if uuid::Uuid::parse_str(id).is_err() || !matches!(extension, "png" | "jpg" | "gif" | "webp") {
        return Err(internal("Invalid stored comics image name"));
    }
    Ok(storage.0.join(album_id.to_string()).join(filename))
}

#[get("/comics/<album_id>/strips/<id>/image")]
async fn strip_image(
    album_id: i32,
    id: i32,
    viewer: Viewer,
    pool: &State<DbPool>,
    storage: &State<ComicStorage>,
) -> ComicResult<NamedFile> {
    let path = {
        let mut conn = pool.get().map_err(internal)?;
        let album = get_album(&mut conn, album_id)?;
        let strip = get_strip(&mut conn, album_id, id)?;
        visible(&viewer, &album, Some(&strip))?;
        image_path(storage, album_id, &strip.image_filename)?
    };
    let file = NamedFile::open(path).await.map_err(internal)?;
    Ok(Private(file))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocket::http::{ContentType, Header};
    use rocket::local::blocking::{Client, LocalResponse};
    use std::io::Cursor;

    fn setup() -> (Client, tempfile::TempDir, DbPool) {
        let dir = tempfile::tempdir().unwrap();
        let pool = crate::db::establish_pool(dir.path().join("test.db").to_str().unwrap());
        {
            let mut conn = pool.get().unwrap();
            crate::db::run_migrations(&mut conn).unwrap();
            let password_hash = bcrypt::hash("password", 4).unwrap();
            for (name, role) in [("alice", "member"), ("bob", "member"), ("admin", "admin")] {
                diesel::insert_into(crate::schema::users::table)
                    .values(crate::models::NewUser {
                        username: name,
                        password_hash: &password_hash,
                        role,
                        created_at: chrono::Utc::now().timestamp(),
                        email: None,
                    })
                    .execute(&mut conn)
                    .unwrap();
            }
        }
        let storage = ComicStorage::initialize(dir.path().join("images")).unwrap();
        let config = rocket::Config {
            limits: rocket::data::Limits::default()
                .limit("file", rocket::data::ByteUnit::MiB * 24)
                .limit("data-form", rocket::data::ByteUnit::MiB * 24),
            ..rocket::Config::debug_default()
        };
        let app = rocket::custom(config)
            .manage(pool.clone())
            .manage(storage)
            .mount("/", routes())
            .mount(
                "/",
                rocket::routes![
                    crate::admin::admin_login_get,
                    crate::admin::admin_login_post,
                    crate::admin::admin_logout
                ],
            )
            .mount("/static", rocket::fs::FileServer::from("static"))
            .register(
                "/",
                rocket::catchers![crate::unauthorized, crate::page_not_found],
            )
            .attach(Template::custom(|engines| {
                engines
                    .tera
                    .register_filter("category", crate::category_filter);
            }));
        (Client::tracked(app).unwrap(), dir, pool)
    }

    fn login(client: &Client, name: &str) -> String {
        let response = client
            .post("/admin/login")
            .header(ContentType::Form)
            .body(format!("username={name}&password=password&next=comics"))
            .dispatch();
        assert_eq!(response.status(), Status::SeeOther);
        assert_eq!(response.headers().get_one("Location"), Some("/comics"));
        let response = client.get("/comics/new").dispatch();
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

    fn create(client: &Client, csrf: &str) -> i32 {
        let response = client
            .post("/comics/new")
            .header(ContentType::Form)
            .body(format!(
                "title=ALICE_ALBUM&subtext=Album+caption&hidden=on&csrf={csrf}"
            ))
            .dispatch();
        assert_eq!(response.status(), Status::SeeOther);
        response
            .headers()
            .get_one("Location")
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap()
            .parse()
            .unwrap()
    }

    fn png() -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(2, 2)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    fn upload<'c>(
        client: &'c Client,
        album: i32,
        csrf: &str,
        hidden: bool,
        bytes: &[u8],
    ) -> LocalResponse<'c> {
        upload_to(
            client,
            &format!("/comics/{album}/strips/new"),
            csrf,
            hidden,
            bytes,
        )
    }

    fn upload_to<'c>(
        client: &'c Client,
        url: &str,
        csrf: &str,
        hidden: bool,
        bytes: &[u8],
    ) -> LocalResponse<'c> {
        let boundary = "comics-test-boundary";
        let mut body = Vec::new();
        let mut fields = vec![
            ("title", "STRIP_TITLE"),
            ("subtext", "Strip caption"),
            ("display_order", "2"),
            ("csrf", csrf),
        ];
        if hidden {
            fields.push(("hidden", "on"));
        }
        for (name, value) in fields {
            body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
        }
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"image\"; filename=\"../../unsafe.svg\"\r\nContent-Type: image/svg+xml\r\n\r\n").as_bytes());
        body.extend_from_slice(bytes);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        client
            .post(url.to_owned())
            .header(Header::new(
                "Content-Type",
                format!("multipart/form-data; boundary={boundary}"),
            ))
            .body(body)
            .dispatch()
    }

    fn publish_album(client: &Client, id: i32, csrf: &str, hidden: bool) {
        let response = client
            .post(format!("/comics/{id}/edit"))
            .header(ContentType::Form)
            .body(format!(
                "title=ALICE_ALBUM&subtext=Album+caption&csrf={csrf}{}",
                if hidden { "&hidden=on" } else { "" }
            ))
            .dispatch();
        assert_eq!(response.status(), Status::SeeOther);
    }

    #[test]
    fn banner_lifecycle_visibility_and_failed_replacement() {
        use diesel::connection::SimpleConnection;
        let (client, dir, pool) = setup();
        let csrf = login(&client, "alice");
        let id = create(&client, &csrf);
        let url = format!("/comics/{id}/banner");
        assert_eq!(client.get(&url).dispatch().status(), Status::NotFound);
        let bytes = png();
        assert_eq!(
            upload_to(&client, &url, &csrf, false, &bytes).status(),
            Status::SeeOther
        );
        let mut conn = pool.get().unwrap();
        let old = get_album(&mut conn, id).unwrap().banner_filename.unwrap();
        let folder = dir.path().join("images").join(id.to_string());
        assert_eq!(std::fs::read(folder.join(&old)).unwrap(), bytes);
        let response = client.get(&url).dispatch();
        assert_eq!(response.status(), Status::Ok);
        assert_eq!(response.content_type(), Some(ContentType::PNG));
        assert_eq!(
            response.headers().get_one("Cache-Control"),
            Some("private, no-store")
        );
        assert_eq!(response.into_bytes().unwrap(), bytes);
        for path in [
            "/comics".to_string(),
            format!("/comics/{id}"),
            format!("/comics/{id}/edit"),
        ] {
            let response = client.get(path).dispatch();
            assert_eq!(response.status(), Status::Ok);
            assert!(
                response
                    .into_string()
                    .unwrap()
                    .contains(&format!("src=\"{url}\""))
            );
        }
        assert_eq!(
            upload_to(&client, &url, "wrong", false, &bytes).status(),
            Status::Forbidden
        );
        assert_eq!(
            upload_to(&client, &url, &csrf, false, b"<svg/>").status(),
            Status::BadRequest
        );
        conn.batch_execute("CREATE TRIGGER reject_banner BEFORE UPDATE ON comic_albums BEGIN SELECT RAISE(ABORT, 'test failure'); END;").unwrap();
        assert_eq!(
            upload_to(&client, &url, &csrf, false, &bytes).status(),
            Status::InternalServerError
        );
        assert_eq!(
            get_album(&mut conn, id).unwrap().banner_filename.as_deref(),
            Some(old.as_str())
        );
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);
        conn.batch_execute("DROP TRIGGER reject_banner;").unwrap();
        assert_eq!(
            upload_to(&client, &url, &csrf, false, &bytes).status(),
            Status::SeeOther
        );
        assert!(!folder.join(&old).exists());
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);
        client.get("/admin/logout").dispatch();
        assert_eq!(client.get(&url).dispatch().status(), Status::NotFound);
        let bob = login(&client, "bob");
        assert_eq!(client.get(&url).dispatch().status(), Status::NotFound);
        assert_eq!(
            upload_to(&client, &url, &bob, false, &bytes).status(),
            Status::NotFound
        );
        let csrf = login(&client, "alice");
        publish_album(&client, id, &csrf, false);
        client.get("/admin/logout").dispatch();
        assert_eq!(client.get(&url).dispatch().status(), Status::Ok);
        let bob = login(&client, "bob");
        assert_eq!(
            upload_to(&client, &url, &bob, false, &bytes).status(),
            Status::Forbidden
        );
        assert_eq!(
            client
                .post(format!("{url}/remove"))
                .header(ContentType::Form)
                .body(format!("csrf={bob}"))
                .dispatch()
                .status(),
            Status::Forbidden
        );
        let admin = login(&client, "admin");
        assert_eq!(
            client
                .post(format!("{url}/remove"))
                .header(ContentType::Form)
                .body("csrf=wrong")
                .dispatch()
                .status(),
            Status::Forbidden
        );
        assert_eq!(
            client
                .post(format!("{url}/remove"))
                .header(ContentType::Form)
                .body(format!("csrf={admin}"))
                .dispatch()
                .status(),
            Status::SeeOther
        );
        assert!(get_album(&mut conn, id).unwrap().banner_filename.is_none());
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
        assert_eq!(client.get(&url).dispatch().status(), Status::NotFound);
        assert_eq!(
            upload_to(&client, &url, &admin, false, &bytes).status(),
            Status::SeeOther
        );
    }

    #[test]
    fn create_upload_publish_and_hide_protects_images() {
        let (client, dir, pool) = setup();
        assert_eq!(client.get("/comics").dispatch().status(), Status::Ok);
        let csrf = login(&client, "alice");
        let id = create(&client, &csrf);
        assert_eq!(
            client.get(format!("/comics/{id}/edit")).dispatch().status(),
            Status::Ok
        );
        assert_eq!(
            client
                .get(format!("/comics/{id}/strips/new"))
                .dispatch()
                .status(),
            Status::Ok
        );
        let bytes = png();
        assert_eq!(
            upload(&client, id, &csrf, true, &bytes).status(),
            Status::SeeOther
        );
        let mut conn = pool.get().unwrap();
        let strip = strips::table.first::<Strip>(&mut conn).unwrap();
        assert!(strip.image_filename.ends_with(".png"));
        assert!(uuid::Uuid::parse_str(strip.image_filename.trim_end_matches(".png")).is_ok());
        assert_eq!(
            std::fs::read(
                dir.path()
                    .join("images")
                    .join(id.to_string())
                    .join(&strip.image_filename)
            )
            .unwrap(),
            bytes
        );
        let image_url = format!("/comics/{id}/strips/{}/image", strip.id);
        let edit_url = format!("/comics/{id}/strips/{}/edit", strip.id);
        let response = client.get(&image_url).dispatch();
        assert_eq!(response.status(), Status::Ok);
        assert_eq!(response.content_type(), Some(ContentType::PNG));
        assert_eq!(
            response.headers().get_one("Cache-Control"),
            Some("private, no-store")
        );
        assert_eq!(
            response.headers().get_one("X-Content-Type-Options"),
            Some("nosniff")
        );
        assert_eq!(response.into_bytes().unwrap(), bytes);
        assert_eq!(client.get(&edit_url).dispatch().status(), Status::Ok);
        let body = client
            .get(format!("/comics/{id}"))
            .dispatch()
            .into_string()
            .unwrap();
        assert!(body.contains("Album caption"));
        assert!(body.contains("Strip caption"));
        assert!(body.contains("Hidden album"));
        publish_album(&client, id, &csrf, false);
        client.get("/admin/logout").dispatch();
        let body = client.get("/comics").dispatch().into_string().unwrap();
        assert!(body.contains("ALICE_ALBUM"));
        let body = client
            .get(format!("/comics/{id}"))
            .dispatch()
            .into_string()
            .unwrap();
        assert!(!body.contains("STRIP_TITLE"));
        assert_eq!(client.get(&image_url).dispatch().status(), Status::NotFound);
        let csrf = login(&client, "alice");
        assert_eq!(
            client
                .post(&edit_url)
                .header(ContentType::Form)
                .body(format!(
                    "title=Published&subtext=Updated+caption&display_order=1&csrf={csrf}"
                ))
                .dispatch()
                .status(),
            Status::SeeOther
        );
        client.get("/admin/logout").dispatch();
        assert_eq!(client.get(&image_url).dispatch().status(), Status::Ok);
        let body = client
            .get(format!("/comics/{id}"))
            .dispatch()
            .into_string()
            .unwrap();
        assert!(body.contains("Updated caption"));
        let csrf = login(&client, "alice");
        publish_album(&client, id, &csrf, true);
        client.get("/admin/logout").dispatch();
        assert_eq!(client.get(&image_url).dispatch().status(), Status::NotFound);
        assert_eq!(
            client.get(format!("/comics/{id}")).dispatch().status(),
            Status::NotFound
        );
        assert!(
            !client
                .get("/comics")
                .dispatch()
                .into_string()
                .unwrap()
                .contains("ALICE_ALBUM")
        );
        assert_eq!(
            client
                .get(format!(
                    "/static/../uploads/comics/{id}/{}",
                    strip.image_filename
                ))
                .dispatch()
                .status(),
            Status::NotFound
        );
    }

    #[test]
    fn ownership_admin_csrf_and_album_strip_binding() {
        let (client, _dir, pool) = setup();
        let csrf = login(&client, "alice");
        let id = create(&client, &csrf);
        let second = create(&client, &csrf);
        assert_eq!(
            upload(&client, id, &csrf, false, &png()).status(),
            Status::SeeOther
        );
        let strip = strips::table
            .first::<Strip>(&mut pool.get().unwrap())
            .unwrap();
        client.get("/admin/logout").dispatch();
        let bob_csrf = login(&client, "bob");
        assert_eq!(
            client.get(format!("/comics/{id}")).dispatch().status(),
            Status::NotFound
        );
        assert_eq!(
            upload(&client, id, &bob_csrf, false, &png()).status(),
            Status::NotFound
        );
        let csrf = login(&client, "alice");
        publish_album(&client, id, &csrf, false);
        let bob_csrf = login(&client, "bob");
        assert_eq!(
            client.get(format!("/comics/{id}/edit")).dispatch().status(),
            Status::Forbidden
        );
        assert_eq!(
            client
                .post(format!("/comics/{id}/edit"))
                .header(ContentType::Form)
                .body(format!("title=Stolen&subtext=&csrf={bob_csrf}"))
                .dispatch()
                .status(),
            Status::Forbidden
        );
        assert_eq!(
            upload(&client, id, &bob_csrf, false, &png()).status(),
            Status::Forbidden
        );
        let csrf = login(&client, "admin");
        assert!(
            client
                .get("/comics")
                .dispatch()
                .into_string()
                .unwrap()
                .contains(&format!("/comics/{second}"))
        );
        assert_eq!(
            client.get(format!("/comics/{second}")).dispatch().status(),
            Status::Ok
        );
        assert_eq!(
            client
                .get(format!("/comics/{second}/strips/{}/image", strip.id))
                .dispatch()
                .status(),
            Status::NotFound
        );
        assert_eq!(
            client
                .get(format!("/comics/{second}/strips/{}/edit", strip.id))
                .dispatch()
                .status(),
            Status::NotFound
        );
        assert_eq!(
            client
                .post(format!("/comics/{id}/edit"))
                .header(ContentType::Form)
                .body("title=bad&subtext=&csrf=wrong")
                .dispatch()
                .status(),
            Status::Forbidden
        );
        assert_eq!(
            upload(&client, second, "wrong", true, &png()).status(),
            Status::Forbidden
        );
        publish_album(&client, second, &csrf, false);
        let album = get_album(&mut pool.get().unwrap(), second).unwrap();
        assert_eq!(album.owner_id, 1);
        assert!(!album.hidden);
        // A signed-in user's current database role, not the role cookie, governs admin access.
        diesel::update(crate::schema::users::table.find(3))
            .set(crate::schema::users::role.eq("member"))
            .execute(&mut pool.get().unwrap())
            .unwrap();
        assert_eq!(
            client.get(format!("/comics/{id}/edit")).dispatch().status(),
            Status::Forbidden
        );
    }

    #[test]
    fn invalid_uploads_and_failed_sql_leave_no_images() {
        use diesel::connection::SimpleConnection;
        let (client, dir, pool) = setup();
        let csrf = login(&client, "alice");
        let id = create(&client, &csrf);
        assert_eq!(
            upload(&client, id, &csrf, true, b"<svg></svg>").status(),
            Status::BadRequest
        );
        assert_eq!(
            upload(&client, id, &csrf, true, b"\x89PNG\r\n\x1a\n").status(),
            Status::BadRequest
        );
        assert_eq!(
            upload(&client, id, &csrf, true, b"").status(),
            Status::PayloadTooLarge
        );
        assert_eq!(
            upload(
                &client,
                id,
                &csrf,
                true,
                &vec![0; MAX_IMAGE_BYTES as usize + 1]
            )
            .status(),
            Status::PayloadTooLarge
        );
        let mut conn = pool.get().unwrap();
        assert_eq!(
            strips::table.count().get_result::<i64>(&mut conn).unwrap(),
            0
        );
        assert!(!dir.path().join("images").join(id.to_string()).exists());
        conn.batch_execute("CREATE TRIGGER reject_strip BEFORE INSERT ON comic_strips BEGIN SELECT RAISE(ABORT, 'test failure'); END;").unwrap();
        assert_eq!(
            upload(&client, id, &csrf, true, &png()).status(),
            Status::InternalServerError
        );
        assert_eq!(
            strips::table.count().get_result::<i64>(&mut conn).unwrap(),
            0
        );
        assert_eq!(
            std::fs::read_dir(dir.path().join("images").join(id.to_string()))
                .unwrap()
                .count(),
            0
        );
        conn.batch_execute("DROP TRIGGER reject_strip; CREATE TRIGGER reject_album_update BEFORE UPDATE ON comic_albums BEGIN SELECT RAISE(ABORT, 'test failure'); END;").unwrap();
        assert_eq!(
            upload(&client, id, &csrf, true, &png()).status(),
            Status::InternalServerError
        );
        assert_eq!(
            strips::table.count().get_result::<i64>(&mut conn).unwrap(),
            0
        );
        assert_eq!(
            std::fs::read_dir(dir.path().join("images").join(id.to_string()))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn image_formats_dimensions_and_size_boundary() {
        let (client, dir, _pool) = setup();
        let csrf = login(&client, "alice");
        let id = create(&client, &csrf);
        for (format, extension) in [
            (image::ImageFormat::Png, "png"),
            (image::ImageFormat::Jpeg, "jpg"),
            (image::ImageFormat::Gif, "gif"),
            (image::ImageFormat::WebP, "webp"),
        ] {
            let mut data = Cursor::new(Vec::new());
            image::DynamicImage::new_rgb8(2, 2)
                .write_to(&mut data, format)
                .unwrap();
            let path = dir.path().join("sample");
            std::fs::write(&path, data.into_inner()).unwrap();
            assert_eq!(validate_image(&path).unwrap(), extension);
        }
        let path = dir.path().join("too-wide.png");
        image::DynamicImage::new_rgba8(16_385, 1)
            .save(&path)
            .unwrap();
        assert_eq!(validate_image(&path).unwrap_err().0, Status::BadRequest);
        let mut bytes = png();
        bytes.resize(MAX_IMAGE_BYTES as usize, 0);
        assert_eq!(
            upload(&client, id, &csrf, true, &bytes).status(),
            Status::SeeOther
        );
    }

    #[test]
    fn ordering_escaped_text_and_validation() {
        let (client, _dir, pool) = setup();
        let csrf = login(&client, "alice");
        let id = create(&client, &csrf);
        assert_eq!(
            client
                .post("/comics/new")
                .header(ContentType::Form)
                .body(format!("title=+&subtext=&csrf={csrf}"))
                .dispatch()
                .status(),
            Status::BadRequest
        );
        assert_eq!(
            upload(&client, id, &csrf, false, &png()).status(),
            Status::SeeOther
        );
        assert_eq!(
            upload(&client, id, &csrf, false, &png()).status(),
            Status::SeeOther
        );
        let items = strips::table
            .order(strips::id.asc())
            .load::<Strip>(&mut pool.get().unwrap())
            .unwrap();
        let edit_url = format!("/comics/{id}/strips/{}/edit", items[1].id);
        assert_eq!(client.post(&edit_url).header(ContentType::Form)
            .body(format!("title=EARLIER&subtext=%3Cscript%3Ealert%281%29%3C%2Fscript%3E&display_order=0&csrf={csrf}")).dispatch().status(), Status::SeeOther);
        let body = client
            .get(format!("/comics/{id}"))
            .dispatch()
            .into_string()
            .unwrap();
        assert!(body.find("EARLIER").unwrap() < body.find("STRIP_TITLE").unwrap());
        assert!(!body.contains("<script>alert(1)</script>"));
        assert!(body.contains("&lt;script&gt;"));
        assert_eq!(
            client
                .post(&edit_url)
                .header(ContentType::Form)
                .body(format!("title=bad&subtext=&display_order=-1&csrf={csrf}"))
                .dispatch()
                .status(),
            Status::BadRequest
        );
        assert!(image_path(&ComicStorage(PathBuf::from("uploads")), id, "../bad.png").is_err());
    }

    #[test]
    fn login_returns_to_comics_without_accepting_arbitrary_redirects() {
        let (client, _dir, _pool) = setup();
        let body = client
            .get("/admin/login?next=comics")
            .dispatch()
            .into_string()
            .unwrap();
        assert!(body.contains("Sign in to Comics"));
        assert!(body.contains("name=\"next\" value=\"comics\""));
        let body = client
            .post("/admin/login")
            .header(ContentType::Form)
            .body("username=alice&password=incorrect&next=comics")
            .dispatch()
            .into_string()
            .unwrap();
        assert!(body.contains("Invalid credentials"));
        assert!(body.contains("name=\"next\" value=\"comics\""));
        let response = client
            .post("/admin/login")
            .header(ContentType::Form)
            .body("username=alice&password=password&next=https%3A%2F%2Fexample.com")
            .dispatch();
        assert_eq!(response.status(), Status::SeeOther);
        assert_eq!(response.headers().get_one("Location"), Some("/admin"));
    }

    #[test]
    fn private_storage_and_expired_sessions() {
        let (client, dir, pool) = setup();
        assert!(ComicStorage::initialize(PathBuf::from("static")).is_err());
        let csrf = login(&client, "alice");
        let id = create(&client, &csrf);
        assert!(
            !client
                .get("/comics")
                .private_cookie(rocket::http::Cookie::new("session_expires", "0"))
                .dispatch()
                .into_string()
                .unwrap()
                .contains("ALICE_ALBUM")
        );
        assert_eq!(
            client
                .get(format!("/comics/{id}"))
                .private_cookie(rocket::http::Cookie::new("session_expires", "0"))
                .dispatch()
                .status(),
            Status::NotFound
        );
        assert_eq!(
            client
                .get("/comics/new")
                .private_cookie(rocket::http::Cookie::new("session_expires", "0"))
                .dispatch()
                .status(),
            Status::Unauthorized
        );
        login(&client, "bob");
        diesel::delete(crate::schema::users::table.find(2))
            .execute(&mut pool.get().unwrap())
            .unwrap();
        assert_eq!(
            client.get("/comics/new").dispatch().status(),
            Status::Unauthorized
        );
        assert!(dir.path().join("images").exists());
    }
}
