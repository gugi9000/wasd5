use super::schema::{
    calendar_allowed_ips, calendar_appointments, calendar_persons, creatine_intakes, packages,
    users,
};
use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;
use serde::Serialize;

#[derive(Queryable, Identifiable, Serialize, Debug)]
#[diesel(table_name = users)]
pub struct User {
    pub id: i32,
    pub username: String,
    pub password_hash: String,
    pub role: String,
    pub created_at: i64,
    pub email: Option<String>,
    pub creatine_reminder: i32,
}

#[derive(Insertable)]
#[diesel(table_name = users)]
pub struct NewUser<'a> {
    pub username: &'a str,
    pub password_hash: &'a str,
    pub role: &'a str,
    pub created_at: i64,
    pub email: Option<&'a str>,
}

#[derive(Queryable, Identifiable, Serialize, Debug, Clone)]
#[diesel(table_name = calendar_persons)]
pub struct CalendarPerson {
    pub id: i32,
    pub name: String,
    pub display_order: i32,
}

#[derive(Insertable)]
#[diesel(table_name = calendar_persons)]
pub struct NewCalendarPerson<'a> {
    pub name: &'a str,
    pub display_order: i32,
}

#[derive(Queryable, Identifiable, Serialize, Debug, Clone)]
#[diesel(table_name = calendar_appointments)]
pub struct CalendarAppointment {
    pub id: i32,
    pub person_id: i32,
    pub title: String,
    pub date: String,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub created_at: i64,
}

#[derive(Insertable)]
#[diesel(table_name = calendar_appointments)]
pub struct NewCalendarAppointment<'a> {
    pub person_id: i32,
    pub title: &'a str,
    pub date: &'a str,
    pub start_time: Option<&'a str>,
    pub end_time: Option<&'a str>,
    pub created_at: i64,
}

#[derive(Queryable, Identifiable, Serialize, Debug, Clone)]
#[diesel(table_name = calendar_allowed_ips)]
pub struct CalendarAllowedIp {
    pub id: i32,
    pub ip_address: String,
    pub created_at: i64,
}

#[derive(Insertable)]
#[diesel(table_name = calendar_allowed_ips)]
pub struct NewCalendarAllowedIp<'a> {
    pub ip_address: &'a str,
    pub created_at: i64,
}

#[derive(Queryable, Identifiable, Serialize, Debug, Clone)]
#[diesel(table_name = packages)]
pub struct Package {
    pub id: i32,
    pub name: String,
    pub ordered_date: i64,
    pub received_date: Option<i64>,
    pub user_id: i32,
    pub tracking_id: Option<String>,
}

#[derive(Insertable)]
#[diesel(table_name = packages)]
pub struct NewPackage {
    pub name: String,
    pub ordered_date: i64,
    pub user_id: i32,
    pub tracking_id: Option<String>,
}

#[derive(Queryable, Identifiable, Serialize, Debug, Clone)]
#[diesel(table_name = creatine_intakes)]
pub struct CreatineIntake {
    pub id: i32,
    pub user_id: i32,
    pub date: String,
    pub amount_grams: f64,
    pub recorded_at: i64,
}

#[derive(Insertable)]
#[diesel(table_name = creatine_intakes)]
pub struct NewCreatineIntake {
    pub user_id: i32,
    pub date: String,
    pub amount_grams: f64,
    pub recorded_at: i64,
}

impl CreatineIntake {
    /// All intakes for a user, ordered newest first.
    pub fn all_for_user(conn: &mut SqliteConnection, uid: i32) -> Vec<CreatineIntake> {
        use super::schema::creatine_intakes::dsl::*;
        creatine_intakes
            .filter(user_id.eq(uid))
            .order(date.asc())
            .load::<CreatineIntake>(conn)
            .unwrap_or_default()
    }

    /// Single intake for a user on a specific date ("YYYY-MM-DD").
    pub fn for_user_on_date(
        conn: &mut SqliteConnection,
        uid: i32,
        d: &str,
    ) -> Option<CreatineIntake> {
        use super::schema::creatine_intakes::dsl::*;
        creatine_intakes
            .filter(user_id.eq(uid).and(date.eq(d)))
            .first::<CreatineIntake>(conn)
            .optional()
            .ok()
            .flatten()
    }

    /// The most recent intake for a user (for defaulting the amount field).
    pub fn last_for_user(conn: &mut SqliteConnection, uid: i32) -> Option<CreatineIntake> {
        use super::schema::creatine_intakes::dsl::*;
        creatine_intakes
            .filter(user_id.eq(uid))
            .order(date.desc())
            .first::<CreatineIntake>(conn)
            .optional()
            .ok()
            .flatten()
    }

    /// Insert or update today's intake (upsert by user_id + date).
    pub fn upsert(conn: &mut SqliteConnection, new: NewCreatineIntake) -> diesel::QueryResult<()> {
        use super::schema::creatine_intakes::dsl::*;
        let existing: Option<CreatineIntake> = creatine_intakes
            .filter(user_id.eq(new.user_id).and(date.eq(&new.date)))
            .first::<CreatineIntake>(conn)
            .optional()?;
        if existing.is_some() {
            diesel::update(
                creatine_intakes.filter(user_id.eq(new.user_id).and(date.eq(&new.date))),
            )
            .set(amount_grams.eq(new.amount_grams))
            .execute(conn)?;
        } else {
            diesel::insert_into(creatine_intakes)
                .values(&new)
                .execute(conn)?;
        }
        Ok(())
    }

    /// Delete the intake for a specific date (owned by the user).
    pub fn delete(conn: &mut SqliteConnection, uid: i32, d: &str) -> diesel::QueryResult<usize> {
        use super::schema::creatine_intakes::dsl::*;
        diesel::delete(creatine_intakes.filter(user_id.eq(uid).and(date.eq(d)))).execute(conn)
    }

    /// Set the creatine_reminder flag for a user.
    pub fn set_reminder(
        conn: &mut SqliteConnection,
        uid: i32,
        enabled: bool,
    ) -> diesel::QueryResult<usize> {
        use super::schema::users::dsl::{creatine_reminder, id, users};
        diesel::update(users.filter(id.eq(uid)))
            .set(creatine_reminder.eq(if enabled { 1 } else { 0 }))
            .execute(conn)
    }
}

impl Package {
    /// All packages for a given user, newest order date first.
    pub fn all_for_user(conn: &mut SqliteConnection, uid: i32) -> Vec<Package> {
        use super::schema::packages::dsl::*;
        packages
            .filter(user_id.eq(uid))
            .order(ordered_date.desc())
            .load::<Package>(conn)
            .unwrap_or_default()
    }

    /// Insert a new package row.
    pub fn create(conn: &mut SqliteConnection, new_pkg: NewPackage) -> diesel::QueryResult<usize> {
        use super::schema::packages::dsl::*;
        diesel::insert_into(packages).values(&new_pkg).execute(conn)
    }

    /// Delete a package only if it belongs to the given user.
    pub fn delete(
        conn: &mut SqliteConnection,
        pkg_id: i32,
        uid: i32,
    ) -> diesel::QueryResult<usize> {
        use super::schema::packages::dsl::*;
        diesel::delete(packages.filter(id.eq(pkg_id).and(user_id.eq(uid)))).execute(conn)
    }

    /// Set `received_date` to `ts` only if the package belongs to the given user.
    pub fn mark_received(
        conn: &mut SqliteConnection,
        pkg_id: i32,
        uid: i32,
        ts: i64,
    ) -> diesel::QueryResult<usize> {
        use super::schema::packages::dsl::*;
        diesel::update(packages.filter(id.eq(pkg_id).and(user_id.eq(uid))))
            .set(received_date.eq(Some(ts)))
            .execute(conn)
    }
}
