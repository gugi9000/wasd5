use super::schema::users;
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
