//! Creatine intake tracking.
//!
//! Routes:
//!   GET  /creatine?year=YYYY&month=M   – calendar view + log form
//!   POST /creatine/log                  – log (or update) today's intake
//!   POST /creatine/delete               – delete a specific day's intake
//!   POST /creatine/reminder             – toggle e-mail reminder flag

use chrono::{Datelike, Duration, NaiveDate, Utc};
use rocket::form::Form;
use rocket::http::CookieJar;
use rocket::response::Redirect;
use rocket::{FromForm, State, get, post};
use rocket_dyn_templates::{Template, context};
use serde::Serialize;
use std::collections::HashMap;

use crate::db::DbPool;
use crate::models::{CreatineIntake, NewCreatineIntake};

// ---------------------------------------------------------------------------
// Session helpers (same pattern as packages.rs)
// ---------------------------------------------------------------------------

fn get_session_user_id(jar: &CookieJar) -> Option<i32> {
    let exp = jar.get_private("session_expires")?;
    let ts: i64 = exp.value().parse().ok()?;
    if ts <= Utc::now().timestamp() {
        return None;
    }
    jar.get_private("user_id")?.value().parse::<i32>().ok()
}

fn valid_csrf(jar: &CookieJar, token: &str) -> bool {
    jar.get_private("csrf")
        .map(|c| c.value() == token)
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Calendar helpers
// ---------------------------------------------------------------------------

/// One cell in the calendar grid.
#[derive(Serialize, Clone)]
pub struct CalendarDay {
    pub day: u32,
    /// "YYYY-MM-DD"
    pub date: String,
    pub in_month: bool,
    pub is_today: bool,
    pub has_intake: bool,
    /// 0.0 when there is no intake recorded
    pub amount: f64,
}

/// Build a Monday-first 6-week calendar grid for the given year/month.
fn build_calendar(
    year: i32,
    month: u32,
    today: &str,
    intakes: &HashMap<String, f64>,
) -> Vec<Vec<CalendarDay>> {
    // Start from the Monday on or before the 1st of the month.
    let first_of_month = NaiveDate::from_ymd_opt(year, month, 1).unwrap_or_default();
    let days_from_monday = first_of_month.weekday().num_days_from_monday() as i64;
    let grid_start = first_of_month - Duration::days(days_from_monday);

    let mut weeks: Vec<Vec<CalendarDay>> = Vec::new();
    let mut cursor = grid_start;

    for _ in 0..6 {
        let mut week: Vec<CalendarDay> = Vec::new();
        for _ in 0..7 {
            let date_str = cursor.format("%Y-%m-%d").to_string();
            let amount = intakes.get(&date_str).copied().unwrap_or(0.0);
            week.push(CalendarDay {
                day: cursor.day(),
                date: date_str.clone(),
                in_month: cursor.month() == month,
                is_today: date_str == today,
                has_intake: amount > 0.0,
                amount,
            });
            cursor += Duration::days(1);
        }
        weeks.push(week);
    }

    weeks
}

// ---------------------------------------------------------------------------
// GET /creatine
// ---------------------------------------------------------------------------

#[get("/creatine?<year>&<month>")]
pub fn creatine_index(
    jar: &CookieJar,
    pool: &State<DbPool>,
    year: Option<i32>,
    month: Option<u32>,
) -> Result<Template, Redirect> {
    let user_id = get_session_user_id(jar).ok_or_else(|| Redirect::to("/admin/login"))?;

    let username = jar
        .get_private("username")
        .map(|c| c.value().to_string())
        .unwrap_or_else(|| "User".to_string());

    let now = Utc::now();
    let today = now.format("%Y-%m-%d").to_string();

    // Use query params or fall back to the current month.
    let view_year = year.unwrap_or(now.year());
    let view_month = month
        .filter(|m| (1..=12).contains(m))
        .unwrap_or(now.month());

    let mut conn = pool.get().map_err(|_| Redirect::to("/"))?;

    // Load all intakes for this user and build a lookup map.
    let all_intakes = CreatineIntake::all_for_user(&mut conn, user_id);
    let intake_map: HashMap<String, f64> = all_intakes
        .iter()
        .map(|i| (i.date.clone(), i.amount_grams))
        .collect();

    // Check today's status.
    let today_intake = CreatineIntake::for_user_on_date(&mut conn, user_id, &today);
    let today_has_intake = today_intake.is_some();
    let today_intake_amount = today_intake.as_ref().map(|i| i.amount_grams).unwrap_or(0.0);

    // Default amount for the form = last recorded amount, else 5.0g.
    let last_amount = CreatineIntake::last_for_user(&mut conn, user_id)
        .map(|i| i.amount_grams)
        .unwrap_or(5.0);

    // Load the user's reminder preference.
    let reminder_enabled = {
        use crate::schema::users::dsl::{id, users};
        use diesel::prelude::*;
        users
            .filter(id.eq(user_id))
            .select(crate::schema::users::dsl::creatine_reminder)
            .first::<i32>(&mut conn)
            .unwrap_or(0)
            == 1
    };

    // Build calendar grid.
    let weeks = build_calendar(view_year, view_month, &today, &intake_map);

    // Month name + navigation links.
    let month_name = NaiveDate::from_ymd_opt(view_year, view_month, 1)
        .map(|d| d.format("%B %Y").to_string())
        .unwrap_or_default();

    let (prev_year, prev_month) = if view_month == 1 {
        (view_year - 1, 12u32)
    } else {
        (view_year, view_month - 1)
    };
    let (next_year, next_month) = if view_month == 12 {
        (view_year + 1, 1u32)
    } else {
        (view_year, view_month + 1)
    };

    let csrf = crate::ensure_csrf(jar);
    let pages = crate::read_pages();

    // Total days taken this month (for a little stat).
    let days_this_month: usize = all_intakes
        .iter()
        .filter(|i| {
            i.date
                .starts_with(&format!("{:04}-{:02}", view_year, view_month))
        })
        .count();

    Ok(Template::render(
        "creatine",
        context! {
            username,
            today,
            today_has_intake,
            today_intake_amount,
            last_amount,
            reminder_enabled,
            year: view_year,
            month: view_month,
            month_name,
            prev_year,
            prev_month,
            next_year,
            next_month,
            weeks,
            days_this_month,
            csrf,
            pages,
        },
    ))
}

// ---------------------------------------------------------------------------
// POST /creatine/log — record (or update) today's intake
// ---------------------------------------------------------------------------

#[derive(FromForm)]
pub struct LogIntakeForm {
    pub amount_grams: f64,
    pub csrf: String,
}

#[post("/creatine/log", data = "<form>")]
pub fn creatine_log(jar: &CookieJar, pool: &State<DbPool>, form: Form<LogIntakeForm>) -> Redirect {
    let Some(user_id) = get_session_user_id(jar) else {
        return Redirect::to("/admin/login");
    };
    let f = form.into_inner();
    if !valid_csrf(jar, &f.csrf) {
        return Redirect::to("/creatine");
    }

    let amount = f.amount_grams.max(0.1);
    let today = Utc::now().format("%Y-%m-%d").to_string();

    if let Ok(mut conn) = pool.get() {
        let new_intake = NewCreatineIntake {
            user_id,
            date: today,
            amount_grams: amount,
            recorded_at: Utc::now().timestamp(),
        };
        let _ = CreatineIntake::upsert(&mut conn, new_intake);
    }

    Redirect::to("/creatine")
}

// ---------------------------------------------------------------------------
// POST /creatine/delete — remove a logged intake
// ---------------------------------------------------------------------------

#[derive(FromForm)]
pub struct DeleteIntakeForm {
    pub date: String,
    pub csrf: String,
}

#[post("/creatine/delete", data = "<form>")]
pub fn creatine_delete(
    jar: &CookieJar,
    pool: &State<DbPool>,
    form: Form<DeleteIntakeForm>,
) -> Redirect {
    let Some(user_id) = get_session_user_id(jar) else {
        return Redirect::to("/admin/login");
    };
    let f = form.into_inner();
    if !valid_csrf(jar, &f.csrf) {
        return Redirect::to("/creatine");
    }

    if let Ok(mut conn) = pool.get() {
        let _ = CreatineIntake::delete(&mut conn, user_id, &f.date);
    }

    Redirect::to("/creatine")
}

// ---------------------------------------------------------------------------
// POST /creatine/reminder — toggle email reminder preference
// ---------------------------------------------------------------------------

#[derive(FromForm)]
pub struct ReminderForm {
    /// 1 = enable, 0 = disable
    pub enabled: i32,
    pub csrf: String,
}

#[post("/creatine/reminder", data = "<form>")]
pub fn creatine_reminder_toggle(
    jar: &CookieJar,
    pool: &State<DbPool>,
    form: Form<ReminderForm>,
) -> Redirect {
    let Some(user_id) = get_session_user_id(jar) else {
        return Redirect::to("/admin/login");
    };
    let f = form.into_inner();
    if !valid_csrf(jar, &f.csrf) {
        return Redirect::to("/creatine");
    }

    if let Ok(mut conn) = pool.get() {
        let _ = CreatineIntake::set_reminder(&mut conn, user_id, f.enabled == 1);
    }

    Redirect::to("/creatine")
}
