use diesel::prelude::*;
use serde_json::{Value, json};
use wasd5::{
    db,
    management::{self, Record, Section},
    models, pages,
    schema::*,
};

fn database() -> SqliteConnection {
    let mut conn = SqliteConnection::establish(":memory:").unwrap();
    db::run_migrations(&mut conn).unwrap();
    let mut admin = management::defaults(Section::Users, "");
    admin["username"] = json!("admin");
    admin["role"] = json!("admin");
    admin["password"] = json!("password");
    management::save(&mut conn, Section::Users, None, admin).unwrap();
    conn
}

fn first(conn: &mut SqliteConnection, section: Section) -> Record {
    management::list(conn, section).unwrap().remove(0)
}

fn create(conn: &mut SqliteConnection, section: Section, changes: Value) -> Record {
    let before: Vec<_> = management::list(conn, section)
        .unwrap()
        .into_iter()
        .map(|r| r.id)
        .collect();
    let mut values = management::defaults(section, "Operator");
    for (key, value) in changes.as_object().unwrap() {
        values[key] = value.clone();
    }
    management::save(conn, section, None, values).unwrap();
    management::list(conn, section)
        .unwrap()
        .into_iter()
        .find(|r| !before.contains(&r.id))
        .unwrap()
}

#[test]
fn gamer_role_is_saved_and_role_changes_revoke_sessions() {
    let mut conn = database();
    let gamer = create(&mut conn, Section::Users,
        json!({"username":"gamer","password":"password","role":"gamer","display_name":"Player"}));
    let user = users::table.find(gamer.id).first::<models::User>(&mut conn).unwrap();
    assert_eq!(user.role, "gamer");
    let mut values = gamer.values.clone();
    values["role"] = json!("member");
    management::save(&mut conn, Section::Users, Some(&gamer), values).unwrap();
    let member = management::list(&mut conn, Section::Users).unwrap().into_iter().find(|row| row.id == gamer.id).unwrap();
    let mut values = member.values.clone();
    values["role"] = json!("gamer");
    management::save(&mut conn, Section::Users, Some(&member), values).unwrap();
    let user = users::table.find(gamer.id).first::<models::User>(&mut conn).unwrap();
    assert_eq!(user.role, "gamer");
    assert_eq!(user.session_version, 2);
    assert!(models::validate_role("super-gamer").is_err());
}

#[test]
fn users_disable_restore_passwords_and_last_admin_protection() {
    let mut conn = database();
    let admin = first(&mut conn, Section::Users);
    assert!(management::remove(&mut conn, Section::Users, &admin, "Operator").is_err());
    let mut demoted = admin.values.clone();
    demoted["role"] = json!("member");
    assert!(management::save(&mut conn, Section::Users, Some(&admin), demoted).is_err());
    create(
        &mut conn,
        Section::Users,
        json!({"username":"member","password":"initial","email":"a@example.org"}),
    );
    let member = management::list(&mut conn, Section::Users)
        .unwrap()
        .into_iter()
        .find(|r| r.values["username"] == "member")
        .unwrap();
    let original: models::User = users::table.find(member.id).first(&mut conn).unwrap();
    assert!(bcrypt::verify("initial", &original.password_hash).unwrap());
    assert!(
        !serde_json::to_string(&member.values)
            .unwrap()
            .contains(&original.password_hash)
    );
    management::remove(&mut conn, Section::Users, &member, "Operator").unwrap();
    let disabled: models::User = users::table.find(member.id).first(&mut conn).unwrap();
    assert!(disabled.disabled);
    assert_eq!(disabled.session_version, 1);
    let disabled = management::list(&mut conn, Section::Users)
        .unwrap()
        .into_iter()
        .find(|r| r.id == member.id)
        .unwrap();
    let mut values = disabled.values.clone();
    values["disabled"] = json!(false);
    values["password"] = json!("changed");
    management::save(&mut conn, Section::Users, Some(&disabled), values).unwrap();
    let restored: models::User = users::table.find(member.id).first(&mut conn).unwrap();
    assert!(!restored.disabled);
    assert_eq!(restored.session_version, 2);
    assert!(bcrypt::verify("changed", &restored.password_hash).unwrap());
    assert!(!bcrypt::verify("initial", &restored.password_hash).unwrap());
    let current = management::list(&mut conn, Section::Users)
        .unwrap()
        .into_iter()
        .find(|r| r.id == member.id)
        .unwrap();
    management::save(
        &mut conn,
        Section::Users,
        Some(&current),
        current.values.clone(),
    )
    .unwrap();
    assert_eq!(
        users::table
            .find(member.id)
            .select(users::password_hash)
            .first::<String>(&mut conn)
            .unwrap(),
        restored.password_hash
    );
}

#[test]
fn pages_share_history_attribution_conflicts_and_hiding() {
    let mut conn = database();
    let page = create(
        &mut conn,
        Section::Pages,
        json!({"slug":"tui/page","content":"# Original"}),
    );
    let mut values = page.values.clone();
    values["content"] = json!("# Edited");
    values["hidden"] = json!(false);
    values["editor"] = json!("Second operator");
    management::save(&mut conn, Section::Pages, Some(&page), values.clone()).unwrap();
    assert!(management::save(&mut conn, Section::Pages, Some(&page), values).is_err());
    let current = first(&mut conn, Section::Pages);
    management::remove(&mut conn, Section::Pages, &current, "Third operator").unwrap();
    let revisions = pages::history(&mut conn, page.id).unwrap();
    assert_eq!(revisions.len(), 3);
    assert_eq!(
        pages::editors(&revisions),
        ["Operator", "Second operator", "Third operator"]
    );
    assert!(pages::by_id(&mut conn, page.id).unwrap().hidden);
    assert!(
        management::details(&mut conn, Section::Pages, &current)
            .unwrap()
            .contains("# Original")
    );
    let current = first(&mut conn, Section::Pages);
    let mut renamed = current.values.clone();
    renamed["slug"] = json!("new/slug");
    assert!(management::save(&mut conn, Section::Pages, Some(&current), renamed).is_err());
}

#[test]
fn calendar_people_appointments_and_allowlist_crud() {
    let mut conn = database();
    let person = create(
        &mut conn,
        Section::People,
        json!({"name":"New person","display_order":0}),
    );
    let appointment = create(
        &mut conn,
        Section::Appointments,
        json!({
            "person_id":person.id,"title":"Appointment","date":"2026-10-30","start_time":"09:00","end_time":"10:00"
        }),
    );
    assert!(management::remove(&mut conn, Section::People, &person, "Operator").is_err());
    let mut updated = appointment.values.clone();
    updated["title"] = json!("Updated appointment");
    management::save(
        &mut conn,
        Section::Appointments,
        Some(&appointment),
        updated,
    )
    .unwrap();
    let current = first(&mut conn, Section::Appointments);
    assert_eq!(current.values["title"], "Updated appointment");
    management::remove(&mut conn, Section::Appointments, &current, "Operator").unwrap();
    management::remove(&mut conn, Section::People, &person, "Operator").unwrap();
    let ip = create(
        &mut conn,
        Section::AllowedIps,
        json!({"ip_address":"2001:0db8::1"}),
    );
    assert_eq!(ip.values["ip_address"], "2001:db8::1");
    management::remove(&mut conn, Section::AllowedIps, &ip, "Operator").unwrap();
    assert!(
        management::list(&mut conn, Section::AllowedIps)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn timers_comics_packages_and_creatine_round_trip() {
    let mut conn = database();
    let timer = create(&mut conn, Section::Timers, json!({"title":"TUI timer"}));
    let mut changed = timer.values.clone();
    changed["workout"]["blocks"][0]["repeats"] = json!(4);
    management::save(&mut conn, Section::Timers, Some(&timer), changed).unwrap();
    let timer = first(&mut conn, Section::Timers);
    assert_eq!(timer.values["workout"]["blocks"][0]["repeats"], 4);
    management::remove(&mut conn, Section::Timers, &timer, "Operator").unwrap();

    let album = create(
        &mut conn,
        Section::Albums,
        json!({"title":"Album","hidden":false}),
    );
    diesel::insert_into(comic_strips::table)
        .values((
            comic_strips::album_id.eq(album.id),
            comic_strips::title.eq("Strip"),
            comic_strips::image_filename.eq("keep.png"),
            comic_strips::display_order.eq(0),
            comic_strips::hidden.eq(false),
            comic_strips::created_at.eq(0),
            comic_strips::updated_at.eq(0),
        ))
        .execute(&mut conn)
        .unwrap();
    let strip = first(&mut conn, Section::Strips);
    let mut edited = strip.values.clone();
    edited["display_order"] = json!(3);
    edited["title"] = json!("Edited strip");
    management::save(&mut conn, Section::Strips, Some(&strip), edited).unwrap();
    let strip = first(&mut conn, Section::Strips);
    management::remove(&mut conn, Section::Strips, &strip, "Operator").unwrap();
    management::remove(&mut conn, Section::Albums, &album, "Operator").unwrap();
    assert!(
        comic_albums::table
            .find(album.id)
            .select(comic_albums::hidden)
            .first::<bool>(&mut conn)
            .unwrap()
    );
    assert!(
        comic_strips::table
            .find(strip.id)
            .select(comic_strips::hidden)
            .first::<bool>(&mut conn)
            .unwrap()
    );
    assert_eq!(
        comic_strips::table
            .find(strip.id)
            .select(comic_strips::image_filename)
            .first::<String>(&mut conn)
            .unwrap(),
        "keep.png"
    );

    let package = create(
        &mut conn,
        Section::Packages,
        json!({"name":"Parcel","ordered_date":"2026-10-01"}),
    );
    let mut received = package.values.clone();
    received["received_date"] = json!("2026-10-30");
    management::save(&mut conn, Section::Packages, Some(&package), received).unwrap();
    let package = first(&mut conn, Section::Packages);
    assert_eq!(package.values["received_date"], "2026-10-30");
    management::remove(&mut conn, Section::Packages, &package, "Operator").unwrap();
    let creatine = create(
        &mut conn,
        Section::Creatine,
        json!({"date":"2026-10-30","amount_grams":5.5}),
    );
    assert_eq!(creatine.values["amount_grams"], 5.5);
    management::remove(&mut conn, Section::Creatine, &creatine, "Operator").unwrap();
}

#[test]
fn invalid_input_or_stale_records_never_overwrite_data() {
    let mut conn = database();
    for (section, changes) in [
        (
            Section::Appointments,
            json!({"date":"2026-02-30","title":"Bad date"}),
        ),
        (
            Section::Appointments,
            json!({"person_id":99999,"title":"Unknown person"}),
        ),
        (
            Section::Appointments,
            json!({"title":"Bad time","start_time":"25:00"}),
        ),
        (
            Section::Appointments,
            json!({"title":"Backwards","start_time":"10:00","end_time":"09:00"}),
        ),
        (
            Section::People,
            json!({"name":"Bad order","display_order":-1}),
        ),
        (Section::AllowedIps, json!({"ip_address":"not-an-ip"})),
        (
            Section::Timers,
            json!({"title":"Bad owner","owner_id":99999}),
        ),
        (
            Section::Timers,
            json!({"title":"Bad workout","workout":{"repeats":0,"blocks":[]}}),
        ),
        (Section::Albums, json!({"title":"","owner_id":1})),
        (
            Section::Packages,
            json!({"name":"Bad dates","ordered_date":"2026-10-30","received_date":"2026-10-29"}),
        ),
        (Section::Creatine, json!({"amount_grams":0})),
        (
            Section::Users,
            json!({"username":"Bad password","password":"x".repeat(73)}),
        ),
    ] {
        let before = management::list(&mut conn, section).unwrap().len();
        let mut values = management::defaults(section, "Operator");
        for (key, value) in changes.as_object().unwrap() {
            values[key] = value.clone();
        }
        assert!(
            management::save(&mut conn, section, None, values).is_err(),
            "{section:?}"
        );
        assert_eq!(management::list(&mut conn, section).unwrap().len(), before);
    }
    let person = first(&mut conn, Section::People);
    let mut values = person.values.clone();
    values["name"] = json!("Newer edit");
    management::save(&mut conn, Section::People, Some(&person), values).unwrap();
    assert!(
        management::save(
            &mut conn,
            Section::People,
            Some(&person),
            person.values.clone()
        )
        .is_err()
    );
    assert!(management::remove(&mut conn, Section::People, &person, "Operator").is_err());
}

#[test]
fn package_timestamp_precision_survives_metadata_edits() {
    let mut conn = database();
    diesel::insert_into(packages::table)
        .values((
            packages::user_id.eq(1),
            packages::name.eq("Parcel"),
            packages::ordered_date.eq(1790812801_i64),
            packages::received_date.eq(Some(1790899999_i64)),
        ))
        .execute(&mut conn)
        .unwrap();
    let package = first(&mut conn, Section::Packages);
    let mut values = package.values.clone();
    values["name"] = json!("Renamed");
    management::save(&mut conn, Section::Packages, Some(&package), values).unwrap();
    let saved: models::Package = packages::table.find(package.id).first(&mut conn).unwrap();
    assert_eq!(saved.ordered_date, 1790812801);
    assert_eq!(saved.received_date, Some(1790899999));
}
