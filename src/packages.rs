use chrono::{NaiveDate, NaiveTime, TimeZone, Utc};
use rocket::form::Form;
use rocket::http::CookieJar;
use rocket::response::Redirect;
use rocket::FromForm;
use rocket::{get, post, State};
use rocket_dyn_templates::{context, Template};
use serde::Serialize;

use crate::db::DbPool;
use crate::models;

// ---------------------------------------------------------------------------
// Session helpers
// ---------------------------------------------------------------------------

/// Returns the logged-in user's ID when there is a valid, non-expired session.
fn get_session_user_id(jar: &CookieJar) -> Option<i32> {
    let exp = jar.get_private("session_expires")?;
    let ts: i64 = exp.value().parse().ok()?;
    if ts <= Utc::now().timestamp() {
        return None;
    }
    let uid = jar.get_private("user_id")?;
    uid.value().parse::<i32>().ok()
}

/// Returns `true` when the CSRF token in the form matches the one in the cookie.
fn valid_csrf(jar: &CookieJar, form_token: &str) -> bool {
    jar.get_private("csrf")
        .map(|c| c.value() == form_token)
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Display helper
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct PackageDisplay {
    id: i32,
    name: String,
    tracking_id: Option<String>,
    ordered_date_str: String,
    received_date_str: Option<String>,
}

fn to_display(p: &models::Package) -> PackageDisplay {
    let fmt_ts = |ts: i64| -> String {
        Utc.timestamp_opt(ts, 0)
            .single()
            .map(|dt| dt.format("%Y-%m-%d").to_string())
            .unwrap_or_default()
    };
    PackageDisplay {
        id: p.id,
        name: p.name.clone(),
        tracking_id: p.tracking_id.clone(),
        ordered_date_str: fmt_ts(p.ordered_date),
        received_date_str: p.received_date.map(fmt_ts),
    }
}

// ---------------------------------------------------------------------------
// GET /packages — list packages for the current user
// ---------------------------------------------------------------------------

#[get("/packages")]
pub fn list_packages(jar: &CookieJar, pool: &State<DbPool>) -> Result<Template, Redirect> {
    let user_id = get_session_user_id(jar).ok_or_else(|| Redirect::to("/admin/login"))?;

    let username = jar
        .get_private("username")
        .map(|c| c.value().to_string())
        .unwrap_or_else(|| "User".to_string());

    let mut conn = pool.get().map_err(|_| Redirect::to("/"))?;
    let pkgs = models::Package::all_for_user(&mut conn, user_id);
    let items: Vec<PackageDisplay> = pkgs.iter().map(to_display).collect();

    let today = Utc::now().format("%Y-%m-%d").to_string();
    let csrf = crate::ensure_csrf(jar);
    let pages = crate::read_pages();

    Ok(Template::render(
        "packages",
        context! { items, today, csrf, pages, username },
    ))
}

// ---------------------------------------------------------------------------
// POST /packages/new — add a package
// ---------------------------------------------------------------------------

#[derive(FromForm)]
pub struct NewPackageForm {
    name: String,
    tracking_id: String,
    ordered_date: String,
    csrf: String,
}

#[post("/packages/new", data = "<form>")]
pub fn create_package(
    jar: &CookieJar,
    pool: &State<DbPool>,
    form: Form<NewPackageForm>,
) -> Redirect {
    let Some(user_id) = get_session_user_id(jar) else {
        return Redirect::to("/admin/login");
    };
    let f = form.into_inner();
    if !valid_csrf(jar, &f.csrf) {
        return Redirect::to("/packages");
    }

    // Parse the user-supplied date; fall back to now if unparseable.
    let ordered_ts = NaiveDate::parse_from_str(f.ordered_date.trim(), "%Y-%m-%d")
        .ok()
        .and_then(|d| {
            d.and_time(NaiveTime::from_hms_opt(12, 0, 0)?)
                .and_utc()
                .timestamp()
                .into()
        })
        .unwrap_or_else(|| Utc::now().timestamp());

    let tracking = {
        let t = f.tracking_id.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    };

    let new_pkg = models::NewPackage {
        name: f.name.trim().to_string(),
        ordered_date: ordered_ts,
        user_id,
        tracking_id: tracking,
    };

    if let Ok(mut conn) = pool.get() {
        let _ = models::Package::create(&mut conn, new_pkg);
    }
    Redirect::to("/packages")
}

// ---------------------------------------------------------------------------
// POST /packages/<id>/delete — remove a package
// ---------------------------------------------------------------------------

#[derive(FromForm)]
pub struct CsrfForm {
    csrf: String,
}

#[post("/packages/<id>/delete", data = "<form>")]
pub fn delete_package(
    jar: &CookieJar,
    pool: &State<DbPool>,
    id: i32,
    form: Form<CsrfForm>,
) -> Redirect {
    let Some(user_id) = get_session_user_id(jar) else {
        return Redirect::to("/admin/login");
    };
    if !valid_csrf(jar, &form.csrf) {
        return Redirect::to("/packages");
    }
    if let Ok(mut conn) = pool.get() {
        let _ = models::Package::delete(&mut conn, id, user_id);
    }
    Redirect::to("/packages")
}

// ---------------------------------------------------------------------------
// POST /packages/<id>/received — mark a package as received today
// ---------------------------------------------------------------------------

#[post("/packages/<id>/received", data = "<form>")]
pub fn mark_received(
    jar: &CookieJar,
    pool: &State<DbPool>,
    id: i32,
    form: Form<CsrfForm>,
) -> Redirect {
    let Some(user_id) = get_session_user_id(jar) else {
        return Redirect::to("/admin/login");
    };
    if !valid_csrf(jar, &form.csrf) {
        return Redirect::to("/packages");
    }
    if let Ok(mut conn) = pool.get() {
        let _ = models::Package::mark_received(&mut conn, id, user_id, Utc::now().timestamp());
    }
    Redirect::to("/packages")
}
