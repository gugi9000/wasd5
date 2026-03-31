use super::schema::{users, calendar_persons, calendar_appointments};
use serde::Serialize;
use diesel::prelude::*;

#[derive(Queryable, Identifiable, Serialize, Debug)]
#[diesel(table_name = users)]
pub struct User {
    pub id: i32,
    pub username: String,
    pub password_hash: String,
    pub role: String,
    pub created_at: i64,
}

#[derive(Insertable)]
#[diesel(table_name = users)]
pub struct NewUser<'a> {
    pub username: &'a str,
    pub password_hash: &'a str,
    pub role: &'a str,
    pub created_at: i64,
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
