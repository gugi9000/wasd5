use diesel::prelude::*;
use serde_json::json;
use wasd5::{
    db,
    management::{self, Section},
    models,
    schema::users,
};

#[test]
fn display_name_creation_editing_fallback_and_validation() {
    let mut conn = SqliteConnection::establish(":memory:").unwrap();
    db::run_migrations(&mut conn).unwrap();
    let mut values = management::defaults(Section::Users, "");
    values["username"] = json!("login-name");
    values["display_name"] = json!("  Visible Name  ");
    values["password"] = json!("password");
    management::save(&mut conn, Section::Users, None, values).unwrap();
    let user = users::table.first::<models::User>(&mut conn).unwrap();
    assert_eq!(user.username, "login-name");
    assert_eq!(user.display_name(), "Visible Name");
    assert_eq!(user.display_name.as_deref(), Some("Visible Name"));
    let record = management::list(&mut conn, Section::Users)
        .unwrap()
        .remove(0);
    assert!(record.summary.starts_with("Visible Name"));
    let mut values = record.values.clone();
    values["display_name"] = json!(" ");
    management::save(&mut conn, Section::Users, Some(&record), values).unwrap();
    let user = users::table.first::<models::User>(&mut conn).unwrap();
    assert_eq!(user.display_name(), "login-name");
    assert!(user.display_name.is_none());
    assert_eq!(user.session_version, 0);
    for name in ["x".repeat(100), "名字".repeat(50)] {
        assert!(models::normalize_display_name(Some(&name)).is_ok());
    }
    for name in ["x".repeat(101), "Visible\nName".into(), "Name\u{0}".into()] {
        assert!(models::normalize_display_name(Some(&name)).is_err());
    }
}

#[test]
fn display_name_migration_backfills_existing_accounts() {
    let mut conn = SqliteConnection::establish(":memory:").unwrap();
    db::run_migrations(&mut conn).unwrap();
    diesel::sql_query(include_str!(
        "../migrations/00000000000021_add_user_display_name/down.sql"
    ))
    .execute(&mut conn)
    .unwrap();
    diesel::insert_into(users::table)
        .values(models::NewUser {
            username: "legacy-name",
            password_hash: "unused",
            role: "member",
            created_at: 1,
            email: None,
        })
        .execute(&mut conn)
        .unwrap();
    use diesel::connection::SimpleConnection;
    conn.batch_execute(include_str!(
        "../migrations/00000000000021_add_user_display_name/up.sql"
    ))
    .unwrap();
    let user = users::table.first::<models::User>(&mut conn).unwrap();
    assert_eq!(user.display_name.as_deref(), Some("legacy-name"));
}
