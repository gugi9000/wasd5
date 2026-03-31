use chrono::{Duration, Utc};
use diesel::prelude::*;
use rocket::http::{CookieJar, Status};
use rocket::response::Redirect;
use rocket::serde::{Deserialize, json::Json};
use rocket::{State, get, post};
use rocket_dyn_templates::{Template, context};

use crate::db::DbPool;
use crate::models;

fn is_ip_allowed(pool: &State<DbPool>, ip: &str) -> bool {
    use crate::schema::calendar_allowed_ips::dsl as aid;
    let mut conn = match pool.get() {
        Ok(c) => c,
        Err(_) => return false,
    };
    aid::calendar_allowed_ips
        .filter(aid::ip_address.eq(ip))
        .first::<models::CalendarAllowedIp>(&mut conn)
        .optional()
        .map(|v| v.is_some())
        .unwrap_or(false)
}

fn can_access_calendar(jar: &CookieJar, pool: &State<DbPool>, remote: &crate::RemoteAddr) -> bool {
    crate::is_admin_cookie(jar) || is_ip_allowed(pool, remote.addr())
}

#[get("/api/calendar/access")]
pub fn api_calendar_access(
    jar: &CookieJar,
    pool: &State<DbPool>,
    remote: crate::RemoteAddr,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "allowed": can_access_calendar(jar, pool, &remote)
    }))
}

#[get("/calendar")]
pub fn calendar_index(
    jar: &CookieJar,
    pool: &State<DbPool>,
    remote: crate::RemoteAddr,
) -> Result<Template, Redirect> {
    if !can_access_calendar(jar, pool, &remote) {
        return Err(Redirect::to("/admin/login"));
    }
    use crate::schema::calendar_persons::dsl as pd;

    let mut conn = pool.get().map_err(|_| Redirect::to("/admin"))?;
    let persons = pd::calendar_persons
        .order(pd::display_order.asc())
        .load::<models::CalendarPerson>(&mut conn)
        .unwrap_or_default();

    let today = Utc::now().date_naive();
    let start_date = today - Duration::days(2);
    let end_date = today + Duration::days(21);
    let start_str = start_date.format("%Y-%m-%d").to_string();
    let end_str = end_date.format("%Y-%m-%d").to_string();

    let pages = crate::read_pages();
    let csrf = crate::ensure_csrf(jar);
    Ok(Template::render(
        "calendar",
        context! {
            persons: persons,
            start_date: start_str,
            end_date: end_str,
            csrf: csrf,
            pages: pages,
        },
    ))
}

#[derive(Deserialize)]
pub struct CalendarApptPayload {
    csrf: String,
    person_id: i32,
    title: String,
    date: String,
    start_time: Option<String>,
    end_time: Option<String>,
}

#[derive(Deserialize)]
pub struct CalendarDeletePayload {
    csrf: String,
}

fn validate_calendar_csrf(jar: &CookieJar, token: &str) -> bool {
    jar.get_private("csrf")
        .map(|c| c.value() == token)
        .unwrap_or(false)
}

fn validate_date(s: &str) -> bool {
    // Must be YYYY-MM-DD with valid numeric parts
    let parts: Vec<&str> = s.splitn(3, '-').collect();
    if parts.len() != 3 {
        return false;
    }
    parts[0].len() == 4
        && parts[1].len() == 2
        && parts[2].len() == 2
        && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit()))
}

fn validate_time(s: &str) -> bool {
    // Must be HH:MM
    let parts: Vec<&str> = s.splitn(2, ':').collect();
    if parts.len() != 2 {
        return false;
    }
    parts[0].len() == 2
        && parts[1].len() == 2
        && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit()))
}

#[get("/api/calendar/appointments?<start>&<end>")]
pub fn api_calendar_get(
    jar: &CookieJar,
    pool: &State<DbPool>,
    remote: crate::RemoteAddr,
    start: Option<String>,
    end: Option<String>,
) -> Result<Json<serde_json::Value>, Status> {
    if !can_access_calendar(jar, pool, &remote) {
        return Err(Status::Unauthorized);
    }
    use crate::schema::calendar_appointments::dsl as ad;
    let mut conn = pool.get().map_err(|_| Status::ServiceUnavailable)?;

    let start_str = start.unwrap_or_else(|| {
        let d = Utc::now().date_naive() - Duration::days(2);
        d.format("%Y-%m-%d").to_string()
    });
    let end_str = end.unwrap_or_else(|| {
        let d = Utc::now().date_naive() + Duration::days(32);
        d.format("%Y-%m-%d").to_string()
    });

    if !validate_date(&start_str) || !validate_date(&end_str) {
        return Err(Status::BadRequest);
    }

    let appointments = ad::calendar_appointments
        .filter(ad::date.ge(&start_str))
        .filter(ad::date.le(&end_str))
        .order(ad::date.asc())
        .load::<models::CalendarAppointment>(&mut conn)
        .map_err(|_| Status::InternalServerError)?;

    Ok(Json(serde_json::json!({ "appointments": appointments })))
}

#[post("/api/calendar/appointments", data = "<payload>")]
pub fn api_calendar_create(
    jar: &CookieJar,
    pool: &State<DbPool>,
    remote: crate::RemoteAddr,
    payload: Json<CalendarApptPayload>,
) -> Result<Json<models::CalendarAppointment>, Status> {
    if !can_access_calendar(jar, pool, &remote) {
        return Err(Status::Unauthorized);
    }
    if !validate_calendar_csrf(jar, &payload.csrf) {
        return Err(Status::Forbidden);
    }
    if !validate_date(&payload.date) {
        return Err(Status::BadRequest);
    }
    if let Some(t) = &payload.start_time {
        if !t.is_empty() && !validate_time(t) {
            return Err(Status::BadRequest);
        }
    }
    if let Some(t) = &payload.end_time {
        if !t.is_empty() && !validate_time(t) {
            return Err(Status::BadRequest);
        }
    }

    use crate::schema::calendar_appointments::dsl as ad;
    let mut conn = pool.get().map_err(|_| Status::ServiceUnavailable)?;

    let start_opt = payload.start_time.as_deref().filter(|s| !s.is_empty());
    let end_opt = payload.end_time.as_deref().filter(|s| !s.is_empty());

    let new = models::NewCalendarAppointment {
        person_id: payload.person_id,
        title: &payload.title,
        date: &payload.date,
        start_time: start_opt,
        end_time: end_opt,
        created_at: Utc::now().timestamp(),
    };
    diesel::insert_into(ad::calendar_appointments)
        .values(&new)
        .execute(&mut conn)
        .map_err(|_| Status::InternalServerError)?;

    let created = ad::calendar_appointments
        .order(ad::id.desc())
        .first::<models::CalendarAppointment>(&mut conn)
        .map_err(|_| Status::InternalServerError)?;
    Ok(Json(created))
}

#[post("/api/calendar/appointments/<id>", data = "<payload>")]
pub fn api_calendar_update(
    id: i32,
    jar: &CookieJar,
    pool: &State<DbPool>,
    remote: crate::RemoteAddr,
    payload: Json<CalendarApptPayload>,
) -> Result<Json<models::CalendarAppointment>, Status> {
    if !can_access_calendar(jar, pool, &remote) {
        return Err(Status::Unauthorized);
    }
    if !validate_calendar_csrf(jar, &payload.csrf) {
        return Err(Status::Forbidden);
    }
    if !validate_date(&payload.date) {
        return Err(Status::BadRequest);
    }
    if let Some(t) = &payload.start_time {
        if !t.is_empty() && !validate_time(t) {
            return Err(Status::BadRequest);
        }
    }
    if let Some(t) = &payload.end_time {
        if !t.is_empty() && !validate_time(t) {
            return Err(Status::BadRequest);
        }
    }

    use crate::schema::calendar_appointments::dsl as ad;
    let mut conn = pool.get().map_err(|_| Status::ServiceUnavailable)?;

    let start_opt: Option<String> = payload.start_time.clone().filter(|s| !s.is_empty());
    let end_opt: Option<String> = payload.end_time.clone().filter(|s| !s.is_empty());

    diesel::update(ad::calendar_appointments.filter(ad::id.eq(id)))
        .set((
            ad::person_id.eq(payload.person_id),
            ad::title.eq(&payload.title),
            ad::date.eq(&payload.date),
            ad::start_time.eq(start_opt),
            ad::end_time.eq(end_opt),
        ))
        .execute(&mut conn)
        .map_err(|_| Status::InternalServerError)?;

    let updated = ad::calendar_appointments
        .filter(ad::id.eq(id))
        .first::<models::CalendarAppointment>(&mut conn)
        .map_err(|_| Status::InternalServerError)?;
    Ok(Json(updated))
}

#[post("/api/calendar/appointments/<id>/delete", data = "<payload>")]
pub fn api_calendar_delete(
    id: i32,
    jar: &CookieJar,
    pool: &State<DbPool>,
    remote: crate::RemoteAddr,
    payload: Json<CalendarDeletePayload>,
) -> Result<Json<serde_json::Value>, Status> {
    if !can_access_calendar(jar, pool, &remote) {
        return Err(Status::Unauthorized);
    }
    if !validate_calendar_csrf(jar, &payload.csrf) {
        return Err(Status::Forbidden);
    }
    use crate::schema::calendar_appointments::dsl as ad;
    let mut conn = pool.get().map_err(|_| Status::ServiceUnavailable)?;
    diesel::delete(ad::calendar_appointments.filter(ad::id.eq(id)))
        .execute(&mut conn)
        .map_err(|_| Status::InternalServerError)?;
    Ok(Json(serde_json::json!({ "deleted": id })))
}