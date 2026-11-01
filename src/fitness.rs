use diesel::prelude::*;
use rocket::http::{CookieJar, Status};
use rocket::serde::json::Json;
use rocket::{State, get, post};
use rocket_dyn_templates::{Template, context};
use serde::{Deserialize, Serialize};

use crate::comics::{Private, Viewer};
use crate::db::DbPool;
use crate::schema::fitness_timers as timers;
use wasd5::workouts::Workout;

type Error = (Status, String);
type Result<T> = std::result::Result<T, Error>;

pub fn routes() -> Vec<rocket::Route> {
    rocket::routes![
        index, new_timer, edit_timer, player, data, create, update, delete
    ]
}

fn bad_input() -> Error {
    (Status::UnprocessableEntity, "Use a title (1-100 characters), description (up to 2,000 characters), 1-50 groups with 1-50 named steps each, repeats from 1-100, and durations from 1-86,400 seconds or manual rest.".into())
}

fn internal(error: impl std::fmt::Display) -> Error {
    eprintln!("Fitness timer operation failed: {error}");
    (
        Status::InternalServerError,
        "The timer could not be loaded or saved. Please try again.".into(),
    )
}

#[derive(Queryable, Serialize)]
struct Timer {
    id: i32,
    owner_id: i32,
    title: String,
    description: String,
    workout: String,
    created_at: i64,
    updated_at: i64,
}

fn find(conn: &mut SqliteConnection, id: i32) -> Result<Timer> {
    timers::table
        .find(id)
        .first(conn)
        .optional()
        .map_err(internal)?
        .ok_or((Status::NotFound, "Timer not found.".into()))
}

fn user(viewer: &Viewer) -> Result<i32> {
    viewer
        .member_identity()
        .map(|(id, _)| id)
        .ok_or((Status::Unauthorized, "Sign in to save timers.".into()))
}

fn can_manage(viewer: &Viewer, timer: &Timer) -> bool {
    viewer
        .member_identity()
        .is_some_and(|(id, admin)| admin || id == timer.owner_id)
}

fn require_manager(viewer: &Viewer, timer: &Timer) -> Result<()> {
    if can_manage(viewer, timer) {
        Ok(())
    } else {
        Err((
            Status::Forbidden,
            "Only the owner or an admin can change this timer.".into(),
        ))
    }
}

fn csrf(jar: &CookieJar<'_>, token: &str) -> Result<()> {
    if jar.get_private("csrf").is_some_and(|c| c.value() == token) {
        Ok(())
    } else {
        Err((
            Status::Forbidden,
            "Invalid form token. Reload the page and try again.".into(),
        ))
    }
}

#[get("/health/timer")]
fn index(
    pool: &State<DbPool>,
    viewer: Viewer,
    navigation: crate::PageNavigation,
) -> Result<Private<Template>> {
    let mut conn = pool.get().map_err(internal)?;
    let timers = timers::table
        .order((timers::updated_at.desc(), timers::id.desc()))
        .load::<Timer>(&mut conn)
        .map_err(internal)?;
    Ok(Private(Template::render(
        "fitness/index",
        context! {
            pages: navigation.0, timers: timers, signed_in: viewer.member_identity().is_some()
        },
    )))
}

#[get("/health/timer/new")]
fn new_timer(
    viewer: Viewer,
    jar: &CookieJar<'_>,
    navigation: crate::PageNavigation,
) -> Result<Private<Template>> {
    user(&viewer)?;
    Ok(Private(Template::render(
        "fitness/editor",
        context! {
            pages: navigation.0, timer_id: Option::<i32>::None, csrf: crate::ensure_csrf(jar)
        },
    )))
}

#[get("/health/timer/<id>/edit")]
fn edit_timer(
    id: i32,
    pool: &State<DbPool>,
    viewer: Viewer,
    jar: &CookieJar<'_>,
    navigation: crate::PageNavigation,
) -> Result<Private<Template>> {
    let timer = find(&mut *pool.get().map_err(internal)?, id)?;
    require_manager(&viewer, &timer)?;
    Ok(Private(Template::render(
        "fitness/editor",
        context! {
            pages: navigation.0, timer_id: id, csrf: crate::ensure_csrf(jar)
        },
    )))
}

#[get("/health/timer/<id>")]
fn player(
    id: i32,
    pool: &State<DbPool>,
    viewer: Viewer,
    jar: &CookieJar<'_>,
    navigation: crate::PageNavigation,
) -> Result<Private<Template>> {
    let timer = find(&mut *pool.get().map_err(internal)?, id)?;
    Ok(Private(Template::render(
        "fitness/player",
        context! {
            pages: navigation.0, timer: &timer, can_manage: can_manage(&viewer, &timer),
            csrf: crate::ensure_csrf(jar)
        },
    )))
}

#[derive(Serialize)]
struct TimerData {
    id: i32,
    title: String,
    description: String,
    workout: Workout,
}

#[get("/health/timer/<id>/data")]
fn data(id: i32, pool: &State<DbPool>) -> Result<Json<TimerData>> {
    let timer = find(&mut *pool.get().map_err(internal)?, id)?;
    let workout: Workout = serde_json::from_str(&timer.workout).map_err(internal)?;
    workout
        .validate()
        .map_err(internal)?;
    Ok(Json(TimerData {
        id,
        title: timer.title,
        description: timer.description,
        workout,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Save {
    csrf: String,
    title: String,
    description: String,
    workout: Workout,
}

impl Save {
    fn validate(&self, jar: &CookieJar<'_>) -> Result<String> {
        csrf(jar, &self.csrf)?;
        if self.title.trim().is_empty()
            || self.title.chars().count() > 100
            || self.description.chars().count() > 2000
        {
            return Err(bad_input());
        }
        self.workout.validate().map_err(|message| (Status::UnprocessableEntity, message.into()))?;
        serde_json::to_string(&self.workout).map_err(internal)
    }
}

#[post("/health/timer", format = "json", data = "<payload>")]
fn create(
    payload: Json<Save>,
    pool: &State<DbPool>,
    viewer: Viewer,
    jar: &CookieJar<'_>,
) -> Result<Json<i32>> {
    let owner = user(&viewer)?;
    let workout = payload.validate(jar)?;
    let mut conn = pool.get().map_err(internal)?;
    let now = chrono::Utc::now().timestamp();
    let id = conn
        .transaction::<i32, diesel::result::Error, _>(|conn| {
            diesel::insert_into(timers::table)
                .values((
                    timers::owner_id.eq(owner),
                    timers::title.eq(payload.title.trim()),
                    timers::description.eq(payload.description.trim()),
                    timers::workout.eq(&workout),
                    timers::created_at.eq(now),
                    timers::updated_at.eq(now),
                ))
                .execute(conn)?;
            timers::table
                .order(timers::id.desc())
                .select(timers::id)
                .first(conn)
        })
        .map_err(internal)?;
    Ok(Json(id))
}

#[post("/health/timer/<id>", format = "json", data = "<payload>")]
fn update(
    id: i32,
    payload: Json<Save>,
    pool: &State<DbPool>,
    viewer: Viewer,
    jar: &CookieJar<'_>,
) -> Result<Json<i32>> {
    let mut conn = pool.get().map_err(internal)?;
    require_manager(&viewer, &find(&mut conn, id)?)?;
    let workout = payload.validate(jar)?;
    diesel::update(timers::table.find(id))
        .set((
            timers::title.eq(payload.title.trim()),
            timers::description.eq(payload.description.trim()),
            timers::workout.eq(workout),
            timers::updated_at.eq(chrono::Utc::now().timestamp()),
        ))
        .execute(&mut conn)
        .map_err(internal)?;
    Ok(Json(id))
}

#[derive(Deserialize)]
struct Delete {
    csrf: String,
}

#[post("/health/timer/<id>/delete", format = "json", data = "<payload>")]
fn delete(
    id: i32,
    payload: Json<Delete>,
    pool: &State<DbPool>,
    viewer: Viewer,
    jar: &CookieJar<'_>,
) -> Result<Status> {
    let mut conn = pool.get().map_err(internal)?;
    require_manager(&viewer, &find(&mut conn, id)?)?;
    csrf(jar, &payload.csrf)?;
    diesel::delete(timers::table.find(id))
        .execute(&mut conn)
        .map_err(internal)?;
    Ok(Status::NoContent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocket::http::ContentType;
    use rocket::local::blocking::Client;
    use serde_json::{Value, json};

    fn setup() -> (Client, tempfile::TempDir, DbPool) {
        let dir = tempfile::tempdir().unwrap();
        let pool = crate::db::establish_pool(dir.path().join("fitness.db").to_str().unwrap());
        {
            let mut conn = pool.get().unwrap();
            crate::db::run_migrations(&mut conn).unwrap();
            for (name, role) in [("alice", "user"), ("bob", "user"), ("admin", "admin")] {
                diesel::insert_into(crate::schema::users::table)
                    .values(crate::models::NewUser {
                        username: name,
                        password_hash: &bcrypt::hash("password", 4).unwrap(),
                        role,
                        created_at: 0,
                        email: None,
                    })
                    .execute(&mut conn)
                    .unwrap();
            }
        }
        let app = rocket::custom(rocket::Config::debug_default())
            .manage(pool.clone())
            .mount("/", routes())
            .mount(
                "/",
                rocket::routes![
                    crate::admin::admin_login_get,
                    crate::admin::admin_login_post,
                    crate::admin::admin_logout,
                ],
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
            .body(format!("username={name}&password=password&next=fitness"))
            .dispatch();
        assert_eq!(response.status(), Status::SeeOther);
        assert_eq!(response.headers().get_one("Location"), Some("/health/timer"));
        let response = client.get("/health/timer/new").dispatch();
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

    fn payload(csrf: &str) -> Value {
        json!({
            "csrf": csrf, "title": "<script>Workout</script>", "description": "Public workout",
            "workout": { "repeats": 2, "blocks": [
                { "repeats": 3, "steps": [
                    { "label": "Work", "seconds": 20, "repeats": 2 },
                    { "label": "Rest", "seconds": null, "repeats": 1 }
                ]}
            ]}
        })
    }

    fn save(client: &Client, csrf: &str) -> i32 {
        let response = client
            .post("/health/timer")
            .header(ContentType::JSON)
            .body(payload(csrf).to_string())
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
        response.into_json().unwrap()
    }

    #[test]
    fn public_library_and_owner_admin_lifecycle() {
        let (client, _dir, pool) = setup();
        assert_eq!(client.get("/health/timer").dispatch().status(), Status::Ok);
        assert_eq!(
            client.get("/health/timer/new").dispatch().status(),
            Status::Unauthorized
        );
        assert_eq!(
            client
                .post("/health/timer")
                .header(ContentType::JSON)
                .body(payload("invalid").to_string())
                .dispatch()
                .status(),
            Status::Unauthorized
        );
        let csrf = login(&client, "alice");
        let id = save(&client, &csrf);
        assert_eq!(
            client
                .get(format!("/health/timer/{id}/edit"))
                .dispatch()
                .status(),
            Status::Ok
        );
        let html = client
            .get(format!("/health/timer/{id}"))
            .dispatch()
            .into_string()
            .unwrap();
        assert!(!html.contains("<script>Workout</script>"));
        assert!(html.contains("Edit timer"));
        assert_eq!(
            client
                .post(format!("/health/timer/{id}"))
                .header(ContentType::JSON)
                .body(payload(&csrf).to_string())
                .dispatch()
                .status(),
            Status::Ok
        );
        assert_eq!(
            timers::table
                .count()
                .get_result::<i64>(&mut pool.get().unwrap())
                .unwrap(),
            1
        );

        client.get("/admin/logout").dispatch();
        for uri in [
            "/health/timer".into(),
            format!("/health/timer/{id}"),
            format!("/health/timer/{id}/data"),
        ] {
            assert_eq!(client.get(uri).dispatch().status(), Status::Ok);
        }
        let value: Value = client
            .get(format!("/health/timer/{id}/data"))
            .dispatch()
            .into_json()
            .unwrap();
        assert_eq!(value["workout"], payload("")["workout"]);
        let html = client
            .get(format!("/health/timer/{id}"))
            .dispatch()
            .into_string()
            .unwrap();
        assert!(!html.contains("Edit timer"));
        assert_eq!(
            client
                .get(format!("/health/timer/{id}/edit"))
                .dispatch()
                .status(),
            Status::Forbidden
        );
        let bob = login(&client, "bob");
        for suffix in ["", "/delete"] {
            let body = if suffix.is_empty() {
                payload(&bob)
            } else {
                json!({"csrf": bob})
            };
            assert_eq!(
                client
                    .post(format!("/health/timer/{id}{suffix}"))
                    .header(ContentType::JSON)
                    .body(body.to_string())
                    .dispatch()
                    .status(),
                Status::Forbidden
            );
        }
        let admin = login(&client, "admin");
        assert_eq!(
            client
                .post(format!("/health/timer/{id}"))
                .header(ContentType::JSON)
                .body(payload(&admin).to_string())
                .dispatch()
                .status(),
            Status::Ok
        );
        assert_eq!(
            client
                .post(format!("/health/timer/{id}/delete"))
                .header(ContentType::JSON)
                .body(json!({"csrf": "wrong"}).to_string())
                .dispatch()
                .status(),
            Status::Forbidden
        );
        assert_eq!(
            client
                .post(format!("/health/timer/{id}/delete"))
                .header(ContentType::JSON)
                .body(json!({"csrf": admin}).to_string())
                .dispatch()
                .status(),
            Status::NoContent
        );
        assert_eq!(
            client
                .get(format!("/health/timer/{id}"))
                .dispatch()
                .status(),
            Status::NotFound
        );
        assert_eq!(
            client
                .get(format!("/health/timer/{id}/data"))
                .dispatch()
                .status(),
            Status::NotFound
        );
    }

    #[test]
    fn invalid_workouts_and_csrf_do_not_write() {
        let (client, _dir, pool) = setup();
        let csrf = login(&client, "alice");
        let mut invalid = Vec::new();
        let original = payload(&csrf);
        for value in [json!(0), json!(101), json!(-1), json!(1.5)] {
            let mut p = original.clone();
            p["workout"]["repeats"] = value;
            invalid.push(p);
        }
        for value in [json!(0), json!(86401)] {
            let mut p = original.clone();
            p["workout"]["blocks"][0]["steps"][0]["seconds"] = value;
            invalid.push(p);
        }
        let mut p = original.clone();
        p["workout"]["blocks"] = json!([]);
        invalid.push(p);
        let mut p = original.clone();
        p["workout"]["blocks"][0]["steps"] = json!([]);
        invalid.push(p);
        let mut p = original.clone();
        p["workout"]["repeats"] = json!(100);
        p["workout"]["blocks"][0]["repeats"] = json!(100);
        invalid.push(p);
        let mut p = original.clone();
        p["workout"]["blocks"][0]["steps"][0]["label"] = json!(" ");
        invalid.push(p);
        for field in ["title", "description"] {
            let mut p = original.clone();
            p[field] = json!("x".repeat(2001));
            invalid.push(p);
        }
        for p in invalid {
            assert_eq!(
                client
                    .post("/health/timer")
                    .header(ContentType::JSON)
                    .body(p.to_string())
                    .dispatch()
                    .status(),
                Status::UnprocessableEntity
            );
        }
        assert_eq!(
            client
                .post("/health/timer")
                .header(ContentType::JSON)
                .body(payload("wrong").to_string())
                .dispatch()
                .status(),
            Status::Forbidden
        );
        assert_eq!(
            timers::table
                .count()
                .get_result::<i64>(&mut pool.get().unwrap())
                .unwrap(),
            0
        );
    }

    #[test]
    fn expired_deleted_users_and_private_controls() {
        let (client, _dir, pool) = setup();
        let csrf = login(&client, "alice");
        let id = save(&client, &csrf);
        let response = client.get(format!("/health/timer/{id}")).dispatch();
        assert_eq!(
            response.headers().get_one("Cache-Control"),
            Some("private, no-store")
        );
        assert_eq!(response.headers().get_one("Vary"), Some("Cookie"));
        assert_eq!(
            client
                .post(format!("/health/timer/{id}"))
                .header(ContentType::JSON)
                .body(payload("wrong").to_string())
                .dispatch()
                .status(),
            Status::Forbidden
        );
        assert_eq!(
            client
                .get("/health/timer/new")
                .private_cookie(rocket::http::Cookie::new("session_expires", "0"))
                .dispatch()
                .status(),
            Status::Unauthorized
        );
        login(&client, "bob");
        diesel::delete(
            crate::schema::users::table.filter(crate::schema::users::username.eq("bob")),
        )
        .execute(&mut pool.get().unwrap())
        .unwrap();
        assert_eq!(
            client.get("/health/timer/new").dispatch().status(),
            Status::Unauthorized
        );
        let csrf = login(&client, "alice");
        assert_eq!(
            client
                .post(format!("/health/timer/{id}/delete"))
                .header(ContentType::JSON)
                .body(json!({"csrf": csrf}).to_string())
                .dispatch()
                .status(),
            Status::NoContent
        );
    }

    #[test]
    fn validation_limits_match_the_player() {
        let mut workout: Workout = serde_json::from_value(payload("")["workout"].clone()).unwrap();
        workout.repeats = 100;
        workout.blocks[0].repeats = 100;
        workout.blocks[0].steps.truncate(1);
        workout.blocks[0].steps[0].repeats = 1;
        workout.blocks[0].steps[0].seconds = Some(86400);
        assert!(workout.validate().is_ok());
        workout.blocks[0].steps[0].repeats = 2;
        assert!(workout.validate().is_err());
        workout.repeats = 1;
        workout.blocks[0].repeats = 0;
        assert!(workout.validate().is_err());
        workout.blocks[0].repeats = 1;
        workout.blocks[0].steps[0].repeats = 0;
        assert!(workout.validate().is_err());
        workout.blocks[0].steps[0].repeats = 1;
        workout.blocks[0].steps[0].label = "x".repeat(101);
        assert!(workout.validate().is_err());
        workout.blocks[0].steps[0].label = "Work".into();
        workout.blocks[0].steps[0].seconds = None;
        assert!(workout.validate().is_ok());
    }
}
