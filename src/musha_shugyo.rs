use diesel::prelude::*;
use rocket::http::{CookieJar, Status};
use rocket::serde::json::Json;
use rocket::{get, post, routes, State};
use rocket_dyn_templates::{context, Template};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::comics::{Private, Viewer};
use crate::db::DbPool;
use crate::schema::musha_shugyo_actions as actions;
use crate::schema::musha_shugyo_journeys as journeys;

type GameError = (Status, String);
type GameResult<T> = Result<T, GameError>;
const GENERATOR_VERSION: i32 = 3;
const MAX_ACTIONS: usize = 100_000;

#[derive(Debug)]
enum ActionError {
    Database(diesel::result::Error),
    Game(GameError),
}

impl From<diesel::result::Error> for ActionError {
    fn from(error: diesel::result::Error) -> Self {
        Self::Database(error)
    }
}

pub fn routes() -> Vec<rocket::Route> {
    routes![index, state, start, act]
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = journeys)]
struct Journey {
    user_id: i32,
    journey_id: String,
    world_month: String,
    world_seed: String,
    generator_version: i32,
    current_turn: i32,
    created_at: i64,
    updated_at: i64,
}

#[derive(Insertable)]
#[diesel(table_name = journeys)]
struct NewJourney<'a> {
    user_id: i32,
    journey_id: &'a str,
    world_month: &'a str,
    world_seed: &'a str,
    generator_version: i32,
    current_turn: i32,
    created_at: i64,
    updated_at: i64,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = actions)]
struct JourneyAction {
    user_id: i32,
    turn: i64,
    action: String,
}

#[derive(Insertable)]
#[diesel(table_name = actions)]
struct NewJourneyAction<'a> {
    user_id: i32,
    turn: i64,
    action: &'a str,
}

#[derive(Deserialize)]
struct StartRequest {
    csrf: String,
}

#[derive(Deserialize)]
struct ActionRequest {
    action: String,
    expected_turn: usize,
    csrf: String,
}

#[derive(Serialize)]
struct JourneyView {
    journey_id: String,
    world_month: String,
    world_seed: String,
    generator_version: i32,
    actions: Vec<String>,
}

#[derive(Serialize)]
struct ActionAccepted {
    turn: usize,
}

fn internal(error: impl std::fmt::Display) -> GameError {
    eprintln!("Musha Shugyo operation failed: {error}");
    (
        Status::InternalServerError,
        "Rejsen kunne ikke indlæses eller gemmes. Prøv igen.".into(),
    )
}

fn require_user(viewer: &Viewer) -> GameResult<i32> {
    viewer.identity().map(|(id, _)| id).ok_or((
        Status::Unauthorized,
        "Log ind for at starte og gemme en rejse.".into(),
    ))
}

fn csrf(jar: &CookieJar<'_>, token: &str) -> GameResult<()> {
    if jar
        .get_private("csrf")
        .is_some_and(|cookie| cookie.value() == token)
    {
        Ok(())
    } else {
        Err((
            Status::Forbidden,
            "Ugyldigt formular-token. Genindlæs siden, og prøv igen.".into(),
        ))
    }
}

fn view(journey: Journey, stored_actions: Vec<JourneyAction>) -> GameResult<JourneyView> {
    if journey.current_turn < 0 || journey.current_turn as usize != stored_actions.len() {
        return Err(internal(
            "Musha Shugyo journey has an inconsistent action count",
        ));
    }
    let actions = stored_actions
        .into_iter()
        .enumerate()
        .map(|(expected_turn, entry)| {
            if entry.user_id != journey.user_id || entry.turn != expected_turn as i64 {
                return Err(internal(
                    "Musha Shugyo journey has a malformed action sequence",
                ));
            }
            Ok(entry.action)
        })
        .collect::<GameResult<Vec<_>>>()?;
    let _ = (journey.user_id, journey.created_at, journey.updated_at);
    Ok(JourneyView {
        journey_id: journey.journey_id,
        world_month: journey.world_month,
        world_seed: journey.world_seed,
        generator_version: journey.generator_version,
        actions,
    })
}

fn append_action(
    conn: &mut diesel::SqliteConnection,
    user_id: i32,
    expected_turn: usize,
    action: &str,
) -> Result<usize, ActionError> {
    let journey = journeys::table
        .find(user_id)
        .first::<Journey>(conn)
        .optional()?
        .ok_or_else(|| {
            ActionError::Game((
                Status::NotFound,
                "Start en rejse, før du vælger en handling.".into(),
            ))
        })?;
    if journey.current_turn as usize != expected_turn {
        return Err(ActionError::Game((
            Status::Conflict,
            "Rejsen er ændret i en anden fane. Genindlæs den nyeste tilstand.".into(),
        )));
    }
    if expected_turn >= MAX_ACTIONS {
        return Err(ActionError::Game((
            Status::Conflict,
            "Denne rejse har nået sin maksimale længde.".into(),
        )));
    }

    let new_action = NewJourneyAction {
        user_id,
        turn: expected_turn as i64,
        action,
    };
    diesel::insert_into(actions::table)
        .values(&new_action)
        .execute(conn)?;
    let updated_at = chrono::Utc::now().timestamp();
    diesel::update(journeys::table.find(user_id))
        .set((
            journeys::current_turn.eq(journey.current_turn + 1),
            journeys::updated_at.eq(updated_at),
        ))
        .execute(conn)?;
    Ok(expected_turn + 1)
}

#[get("/games/musha-shugyo/api/state")]
fn state(pool: &State<DbPool>, viewer: Viewer) -> GameResult<Private<Json<Option<JourneyView>>>> {
    let user_id = require_user(&viewer)?;
    let mut conn = pool.get().map_err(internal)?;
    let journey = conn
        .transaction::<_, ActionError, _>(|conn| {
            let journey = journeys::table
                .find(user_id)
                .first::<Journey>(conn)
                .optional()?;
            journey
                .map(|journey| {
                    let stored_actions = actions::table
                        .filter(actions::user_id.eq(user_id))
                        .order(actions::turn.asc())
                        .load(conn)?;
                    view(journey, stored_actions).map_err(ActionError::Game)
                })
                .transpose()
        })
        .map_err(|error| match error {
            ActionError::Database(error) => internal(error),
            ActionError::Game(error) => error,
        })?;
    Ok(Private(Json(journey)))
}

#[post("/games/musha-shugyo/api/journey", format = "json", data = "<request>")]
fn start(
    pool: &State<DbPool>,
    jar: &CookieJar<'_>,
    viewer: Viewer,
    request: Json<StartRequest>,
) -> GameResult<Private<Json<JourneyView>>> {
    csrf(jar, &request.csrf)?;
    let user_id = require_user(&viewer)?;
    let mut conn = pool.get().map_err(internal)?;
    let (journey, stored_actions) = conn
        .immediate_transaction::<_, ActionError, _>(|conn| {
            let journey = if let Some(journey) = journeys::table
                .find(user_id)
                .first::<Journey>(conn)
                .optional()?
            {
                journey
            } else {
                let now = chrono::Utc::now();
                let created_at = now.timestamp();
                let world_month = now.format("%Y-%m").to_string();
                let world_seed = format!("musha-shugyo:{world_month}");
                let journey_id = Uuid::new_v4().as_hyphenated().to_string();
                let new_journey = NewJourney {
                    user_id,
                    journey_id: &journey_id,
                    world_month: &world_month,
                    world_seed: &world_seed,
                    generator_version: GENERATOR_VERSION,
                    current_turn: 0,
                    created_at,
                    updated_at: created_at,
                };
                diesel::insert_into(journeys::table)
                    .values(&new_journey)
                    .execute(conn)?;
                journeys::table.find(user_id).first::<Journey>(conn)?
            };
            let stored_actions = actions::table
                .filter(actions::user_id.eq(user_id))
                .order(actions::turn.asc())
                .load(conn)?;
            Ok((journey, stored_actions))
        })
        .map_err(|error| match error {
            ActionError::Database(error) => internal(error),
            ActionError::Game(error) => error,
        })?;
    Ok(Private(Json(view(journey, stored_actions)?)))
}

#[post("/games/musha-shugyo/api/action", format = "json", data = "<request>")]
fn act(
    pool: &State<DbPool>,
    jar: &CookieJar<'_>,
    viewer: Viewer,
    request: Json<ActionRequest>,
) -> GameResult<Private<Json<ActionAccepted>>> {
    csrf(jar, &request.csrf)?;
    let user_id = require_user(&viewer)?;
    const ALLOWED_ACTIONS: &[&str] = &[
        "north",
        "east",
        "south",
        "west",
        "wait",
        "observe",
        "attack",
        "flee",
        "upgrade_rules",
        "collect",
        "activate_checkpoint",
        "train",
        "return_checkpoint",
        "yield",
    ];
    if !ALLOWED_ACTIONS.contains(&request.action.as_str()) {
        return Err((Status::UnprocessableEntity, "Ukendt handling.".into()));
    }

    let mut conn = pool.get().map_err(internal)?;
    let result = conn
        .immediate_transaction::<_, ActionError, _>(|conn| {
            append_action(conn, user_id, request.expected_turn, &request.action)
        })
        .map_err(|error| match error {
            ActionError::Database(error) => internal(error),
            ActionError::Game(error) => error,
        })?;

    Ok(Private(Json(ActionAccepted { turn: result })))
}

#[get("/games/musha-shugyo")]
fn index(
    viewer: Viewer,
    navigation: crate::PageNavigation,
    jar: &CookieJar<'_>,
) -> Private<Template> {
    Private(Template::render(
        "games/musha_shugyo",
        context! {
            pages: navigation.0,
            csrf: crate::ensure_csrf(jar),
            signed_in: viewer.identity().is_some(),
            username: viewer.display_name().unwrap_or(""),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::schema::users;
    use rocket::http::ContentType;
    use rocket::local::blocking::Client;

    fn test_database() -> (diesel::SqliteConnection, i32) {
        let mut conn = diesel::SqliteConnection::establish(":memory:").unwrap();
        db::run_migrations(&mut conn).unwrap();
        diesel::insert_into(users::table)
            .values((
                users::username.eq("musha-player"),
                users::password_hash.eq("unused"),
                users::role.eq("member"),
                users::created_at.eq(1_i64),
            ))
            .execute(&mut conn)
            .unwrap();
        let user_id = users::table
            .filter(users::username.eq("musha-player"))
            .select(users::id)
            .first(&mut conn)
            .unwrap();
        (conn, user_id)
    }

    fn insert_test_journey(conn: &mut diesel::SqliteConnection, user_id: i32) {
        diesel::insert_into(journeys::table)
            .values(NewJourney {
                user_id,
                journey_id: "journey-one",
                world_month: "2026-10",
                world_seed: "musha-shugyo:2026-10",
                generator_version: GENERATOR_VERSION,
                current_turn: 0,
                created_at: 1,
                updated_at: 1,
            })
            .execute(conn)
            .unwrap();
    }

    fn api_clients() -> (Client, Client, Client, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let database_path = dir.path().join("musha.sqlite");
        let pool = db::establish_pool(database_path.to_str().unwrap());
        {
            let mut conn = pool.get().unwrap();
            db::run_migrations(&mut conn).unwrap();
            let password_hash = bcrypt::hash("password", 4).unwrap();
            for username in ["musha-player", "another-player"] {
                diesel::insert_into(users::table)
                    .values((
                        users::username.eq(username),
                        users::password_hash.eq(&password_hash),
                        users::role.eq("member"),
                        users::created_at.eq(1_i64),
                    ))
                    .execute(&mut conn)
                    .unwrap();
            }
        }
        let client_for = |pool: DbPool| {
            Client::tracked(
                rocket::custom(rocket::Config::debug_default())
                    .attach(wasd5::auth::sessions())
                    .attach(Template::fairing())
                    .manage(pool)
                    .mount(
                        "/",
                        routes![
                            crate::admin::admin_login_get,
                            crate::admin::admin_login_post
                        ],
                    )
                    .mount("/", super::routes()),
            )
            .unwrap()
        };
        let guest = client_for(pool.clone());
        let first = client_for(pool.clone());
        let second = client_for(pool);
        login(&first, "musha-player");
        login(&second, "another-player");
        (guest, first, second, dir)
    }

    fn login(client: &Client, username: &str) {
        let response = client
            .post("/admin/login")
            .header(ContentType::Form)
            .body(format!("username={username}&password=password"))
            .dispatch();
        assert_eq!(response.status(), Status::SeeOther);
    }

    fn csrf_for(client: &Client) -> String {
        let response = client.get("/games/musha-shugyo").dispatch();
        assert_eq!(response.status(), Status::Ok);
        let html = response.into_string().unwrap();
        assert!(html.contains("data-signed-in=\"true\""));
        html.split("data-csrf=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .to_string()
    }

    fn post_json<'a>(
        client: &'a Client,
        url: &'a str,
        body: serde_json::Value,
    ) -> rocket::local::blocking::LocalResponse<'a> {
        client
            .post(url)
            .header(ContentType::JSON)
            .body(body.to_string())
            .dispatch()
    }

    #[test]
    fn a_user_can_have_only_one_persistent_journey() {
        let (mut conn, user_id) = test_database();
        insert_test_journey(&mut conn, user_id);
        let duplicate = diesel::insert_into(journeys::table)
            .values(NewJourney {
                user_id,
                journey_id: "journey-two",
                world_month: "2026-10",
                world_seed: "musha-shugyo:2026-10",
                generator_version: GENERATOR_VERSION,
                current_turn: 0,
                created_at: 2,
                updated_at: 2,
            })
            .execute(&mut conn);
        assert!(duplicate.is_err());
    }

    #[test]
    fn generator_version_is_pinned_when_a_journey_is_created() {
        assert_eq!(GENERATOR_VERSION, 3);
        let month = "2026-10";
        assert_eq!(format!("musha-shugyo:{month}"), "musha-shugyo:2026-10");
    }

    #[test]
    fn journey_actions_are_atomic_and_reject_stale_turns() {
        let (mut conn, user_id) = test_database();
        insert_test_journey(&mut conn, user_id);

        let accepted = conn
            .immediate_transaction::<_, ActionError, _>(|conn| {
                append_action(conn, user_id, 0, "wait")
            })
            .unwrap();
        assert_eq!(accepted, 1);

        let stale = conn.immediate_transaction::<_, ActionError, _>(|conn| {
            append_action(conn, user_id, 0, "observe")
        });
        assert!(matches!(
            stale,
            Err(ActionError::Game((status, _))) if status == Status::Conflict
        ));
        assert_eq!(
            actions::table
                .filter(actions::user_id.eq(user_id))
                .count()
                .get_result::<i64>(&mut conn)
                .unwrap(),
            1
        );
        assert_eq!(
            journeys::table
                .find(user_id)
                .select(journeys::current_turn)
                .first::<i32>(&mut conn)
                .unwrap(),
            1
        );
    }

    #[test]
    fn api_requires_accounts_and_persists_one_monthly_journey_per_user() {
        let (guest, first, second, _dir) = api_clients();
        assert_eq!(
            guest
                .get("/games/musha-shugyo/api/state")
                .dispatch()
                .status(),
            Status::Unauthorized
        );
        let first_csrf = csrf_for(&first);
        let second_csrf = csrf_for(&second);
        let empty_state: serde_json::Value = first
            .get("/games/musha-shugyo/api/state")
            .dispatch()
            .into_json()
            .unwrap();
        assert!(empty_state.is_null());

        assert_eq!(
            post_json(
                &first,
                "/games/musha-shugyo/api/journey",
                serde_json::json!({"csrf":"wrong"})
            )
            .status(),
            Status::Forbidden
        );
        let first_journey: serde_json::Value = post_json(
            &first,
            "/games/musha-shugyo/api/journey",
            serde_json::json!({"csrf":&first_csrf}),
        )
        .into_json()
        .unwrap();
        let same_journey: serde_json::Value = post_json(
            &first,
            "/games/musha-shugyo/api/journey",
            serde_json::json!({"csrf":&first_csrf}),
        )
        .into_json()
        .unwrap();
        let second_journey: serde_json::Value = post_json(
            &second,
            "/games/musha-shugyo/api/journey",
            serde_json::json!({"csrf":&second_csrf}),
        )
        .into_json()
        .unwrap();

        assert_eq!(first_journey["journey_id"], same_journey["journey_id"]);
        assert_ne!(first_journey["journey_id"], second_journey["journey_id"]);
        assert_eq!(first_journey["world_seed"], second_journey["world_seed"]);
        assert_eq!(first_journey["generator_version"], GENERATOR_VERSION);

        let accepted: serde_json::Value = post_json(
            &first,
            "/games/musha-shugyo/api/action",
            serde_json::json!({
                "action":"wait",
                "expected_turn":0,
                "csrf":&first_csrf
            }),
        )
        .into_json()
        .unwrap();
        assert_eq!(accepted["turn"], 1);
        let accepted_attack: serde_json::Value = post_json(
            &first,
            "/games/musha-shugyo/api/action",
            serde_json::json!({
                "action":"attack",
                "expected_turn":1,
                "csrf":&first_csrf
            }),
        )
        .into_json()
        .unwrap();
        assert_eq!(accepted_attack["turn"], 2);
        let accepted_flee: serde_json::Value = post_json(
            &first,
            "/games/musha-shugyo/api/action",
            serde_json::json!({
                "action":"flee",
                "expected_turn":2,
                "csrf":&first_csrf
            }),
        )
        .into_json()
        .unwrap();
        assert_eq!(accepted_flee["turn"], 3);
        let accepted_training: serde_json::Value = post_json(
            &first,
            "/games/musha-shugyo/api/action",
            serde_json::json!({
                "action":"train",
                "expected_turn":3,
                "csrf":&first_csrf
            }),
        )
        .into_json()
        .unwrap();
        assert_eq!(accepted_training["turn"], 4);
        let accepted_upgrade: serde_json::Value = post_json(
            &first,
            "/games/musha-shugyo/api/action",
            serde_json::json!({
                "action":"upgrade_rules",
                "expected_turn":4,
                "csrf":&first_csrf
            }),
        )
        .into_json()
        .unwrap();
        assert_eq!(accepted_upgrade["turn"], 5);
        assert_eq!(
            post_json(
                &first,
                "/games/musha-shugyo/api/action",
                serde_json::json!({
                    "action":"observe",
                    "expected_turn":0,
                    "csrf":&first_csrf
                }),
            )
            .status(),
            Status::Conflict
        );
        assert_eq!(
            post_json(
                &first,
                "/games/musha-shugyo/api/action",
                serde_json::json!({
                    "action":"erase_journey",
                    "expected_turn":5,
                    "csrf":&first_csrf
                }),
            )
            .status(),
            Status::UnprocessableEntity
        );
        let state: serde_json::Value = first
            .get("/games/musha-shugyo/api/state")
            .dispatch()
            .into_json()
            .unwrap();
        assert_eq!(
            state["actions"],
            serde_json::json!(["wait", "attack", "flee", "train", "upgrade_rules"])
        );
    }
}
