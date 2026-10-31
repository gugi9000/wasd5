use diesel::prelude::*;
use rocket::http::{CookieJar, Status};
use rocket::serde::json::Json;
use rocket::{State, get, post};
use rocket_dyn_templates::{Template, context};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::comics::{Private, Viewer};
use crate::db::DbPool;
use crate::schema::{tetris_runs as runs, tetris_scores as scores};

type TetrisError = (Status, String);
type TetrisResult<T> = Result<T, TetrisError>;

enum SubmitError {
    Database(diesel::result::Error),
    Request(TetrisError),
}

impl From<diesel::result::Error> for SubmitError {
    fn from(error: diesel::result::Error) -> Self {
        Self::Database(error)
    }
}

pub fn routes() -> Vec<rocket::Route> {
    rocket::routes![index, start, submit_score, scores_json]
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = runs)]
struct Run {
    user_id: Option<i32>,
    created_at: i64,
    submitted_at: Option<i64>,
}

#[derive(Insertable)]
#[diesel(table_name = runs)]
struct NewRun<'a> {
    id: &'a str,
    user_id: Option<i32>,
    created_at: i64,
    submitted_at: Option<i64>,
}

#[derive(Queryable, Serialize)]
struct Score {
    id: i32,
    run_id: String,
    player_name: String,
    user_id: Option<i32>,
    score: i64,
    lines: i32,
    level: i32,
    created_at: i64,
}

#[derive(Deserialize)]
struct StartRequest {
    csrf: String,
}

#[derive(Deserialize)]
struct ScoreRequest {
    player_name: Option<String>,
    score: i64,
    lines: i32,
    level: i32,
    csrf: String,
}

fn internal(error: impl std::fmt::Display) -> TetrisError {
    eprintln!("Tetris operation failed: {error}");
    (
        Status::InternalServerError,
        "The Tetris score could not be saved or loaded. Please try again.".into(),
    )
}

fn csrf(jar: &CookieJar<'_>, token: &str) -> TetrisResult<()> {
    if jar.get_private("csrf").is_some_and(|c| c.value() == token) {
        Ok(())
    } else {
        Err((
            Status::Forbidden,
            "Invalid form token. Reload the page and try again.".into(),
        ))
    }
}

fn leaderboard(conn: &mut diesel::SqliteConnection) -> TetrisResult<Vec<Score>> {
    let mut scores = scores::table
        .order((scores::score.desc(), scores::created_at.asc()))
        .limit(10)
        .load::<Score>(conn)
        .map_err(internal)?;
    let names = crate::models::account_display_names(conn, scores.iter().filter_map(|score| score.user_id)).map_err(internal)?;
    for score in &mut scores {
        if let Some(name) = score.user_id.and_then(|id| names.get(&id)) {
            score.player_name.clone_from(name);
        }
    }
    Ok(scores)
}

#[get("/games/tetris")]
fn index(
    pool: &State<DbPool>,
    viewer: Viewer,
    navigation: crate::PageNavigation,
    jar: &CookieJar<'_>,
) -> TetrisResult<Private<Template>> {
    let mut conn = pool.get().map_err(internal)?;
    let leaderboard = leaderboard(&mut conn)?;
    Ok(Private(Template::render(
        "games/tetris",
        context! {
            pages: navigation.0,
            csrf: crate::ensure_csrf(jar),
            signed_in: viewer.identity().is_some(),
            username: viewer.display_name().unwrap_or(""),
            leaderboard: leaderboard,
        },
    )))
}

#[post("/games/tetris/api/runs", format = "json", data = "<request>")]
fn start(
    pool: &State<DbPool>,
    jar: &CookieJar<'_>,
    viewer: Viewer,
    request: Json<StartRequest>,
) -> TetrisResult<Private<Json<serde_json::Value>>> {
    csrf(jar, &request.csrf)?;
    let id = Uuid::new_v4().as_hyphenated().to_string();
    let now = chrono::Utc::now().timestamp_millis();
    let new_run = NewRun {
        id: &id,
        user_id: viewer.identity().map(|(id, _)| id),
        created_at: now,
        submitted_at: None,
    };
    let mut conn = pool.get().map_err(internal)?;
    diesel::delete(runs::table.filter(runs::created_at.lt(now - 86_400_000)))
        .execute(&mut conn)
        .map_err(internal)?;
    diesel::insert_into(runs::table)
        .values(&new_run)
        .execute(&mut conn)
        .map_err(internal)?;
    Ok(Private(Json(serde_json::json!({ "id": id }))))
}

#[post(
    "/games/tetris/api/runs/<id>/score",
    format = "json",
    data = "<request>"
)]
fn submit_score(
    id: &str,
    pool: &State<DbPool>,
    jar: &CookieJar<'_>,
    viewer: Viewer,
    request: Json<ScoreRequest>,
) -> TetrisResult<Private<Json<Score>>> {
    csrf(jar, &request.csrf)?;
    let input = request.into_inner();
    if !(0..=100_000_000).contains(&input.score)
        || !(0..=100_000).contains(&input.lines)
        || !(1..=10_001).contains(&input.level)
    {
        return Err((
            Status::UnprocessableEntity,
            "Score, lines, or level is outside the allowed range.".into(),
        ));
    }
    let identity = viewer.identity().map(|(id, _)| id);
    let player_name = if let Some(username) = viewer.display_name() {
        username.to_owned()
    } else {
        let name = input.player_name.as_deref().unwrap_or("").trim();
        if name.is_empty() || name.chars().count() > 24 || name.chars().any(char::is_control) {
            return Err((
                Status::UnprocessableEntity,
                "Enter a name of 1–24 characters without control characters.".into(),
            ));
        }
        name.to_owned()
    };

    let mut conn = pool.get().map_err(internal)?;
    let result = conn.transaction::<Score, SubmitError, _>(|conn| {
        let run = runs::table
            .find(&id)
            .select(Run::as_select())
            .first::<Run>(conn)
            .optional()?
            .ok_or_else(|| {
                SubmitError::Request((Status::NotFound, "Game not found or expired.".into()))
            })?;
        if run.created_at < chrono::Utc::now().timestamp_millis() - 86_400_000 {
            return Err(SubmitError::Request((
                Status::NotFound,
                "Game not found or expired.".into(),
            )));
        }
        if run.user_id != identity {
            return Err(SubmitError::Request((
                Status::Forbidden,
                "This game belongs to a different player.".into(),
            )));
        }
        if run.submitted_at.is_some() {
            return Err(SubmitError::Request((
                Status::Conflict,
                "This game has already been submitted.".into(),
            )));
        }
        let now = chrono::Utc::now().timestamp_millis();
        if now - run.created_at < 10_000 {
            return Err(SubmitError::Request((
                Status::UnprocessableEntity,
                "Play for at least 10 seconds before submitting a score.".into(),
            )));
        }
        diesel::update(runs::table.find(&id))
            .set(runs::submitted_at.eq(Some(now)))
            .execute(conn)?;
        diesel::insert_into(scores::table)
            .values((
                scores::run_id.eq(&id),
                scores::player_name.eq(player_name),
                scores::user_id.eq(identity),
                scores::score.eq(input.score),
                scores::lines.eq(input.lines),
                scores::level.eq(input.level),
                scores::created_at.eq(now),
            ))
            .execute(conn)?;
        scores::table
            .filter(scores::run_id.eq(&id))
            .first::<Score>(conn)
            .map_err(SubmitError::Database)
    });
    match result {
        Ok(score) => Ok(Private(Json(score))),
        Err(SubmitError::Request(error)) => Err(error),
        Err(SubmitError::Database(error)) => Err(internal(error)),
    }
}

#[get("/games/tetris/api/scores")]
fn scores_json(pool: &State<DbPool>) -> TetrisResult<Private<Json<Vec<Score>>>> {
    let mut conn = pool.get().map_err(internal)?;
    Ok(Private(Json(leaderboard(&mut conn)?)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use rocket::http::ContentType;
    use rocket::local::blocking::Client;

    #[test]
    fn account_scoreboards_resolve_current_display_names_without_changing_guests() {
        let mut conn = diesel::SqliteConnection::establish(":memory:").unwrap();
        db::run_migrations(&mut conn).unwrap();
        diesel::insert_into(crate::schema::users::table).values((
            crate::models::NewUser {
                username: "login", password_hash: "unused", role: "member", created_at: 1, email: None,
            },
            crate::schema::users::display_name.eq(Some("Visible Player")),
        )).execute(&mut conn).unwrap();
        let user_id = crate::schema::users::table.select(crate::schema::users::id).first::<i32>(&mut conn).unwrap();
        for (run_id, user_id, player_name) in [("account", Some(user_id), "login"), ("guest", None, "Guest")] {
            diesel::insert_into(runs::table).values(NewRun {
                id: run_id, user_id, created_at: 1, submitted_at: Some(2),
            }).execute(&mut conn).unwrap();
            diesel::insert_into(scores::table).values((
                scores::run_id.eq(run_id), scores::user_id.eq(user_id), scores::player_name.eq(player_name),
                scores::score.eq(100), scores::lines.eq(1), scores::level.eq(1), scores::created_at.eq(2),
            )).execute(&mut conn).unwrap();
        }
        let board = leaderboard(&mut conn).unwrap();
        assert_eq!(board[0].player_name, "Visible Player");
        assert_eq!(board[1].player_name, "Guest");
        diesel::update(crate::schema::users::table.find(user_id))
            .set(crate::schema::users::display_name.eq(Some("Renamed Player"))).execute(&mut conn).unwrap();
        assert_eq!(leaderboard(&mut conn).unwrap()[0].player_name, "Renamed Player");
        diesel::update(crate::schema::users::table.find(user_id))
            .set(crate::schema::users::display_name.eq(None::<String>)).execute(&mut conn).unwrap();
        assert_eq!(leaderboard(&mut conn).unwrap()[0].player_name, "login");
    }

    #[test]
    fn guest_can_submit_a_run_once_and_leaderboard_sorts_highest_first() {
        let dir = tempfile::tempdir().unwrap();
        let pool = db::establish_pool(dir.path().join("tetris.db").to_str().unwrap());
        let mut conn = pool.get().unwrap();
        db::run_migrations(&mut conn).unwrap();
        let app = rocket::custom(rocket::Config::debug_default())
            .attach(wasd5::auth::sessions())
            .manage(pool.clone())
            .mount("/", routes())
            .attach(Template::fairing());
        let client = Client::tracked(app).unwrap();
        let html = client
            .get("/games/tetris")
            .dispatch()
            .into_string()
            .unwrap();
        let csrf = html
            .split("data-csrf=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let run: serde_json::Value = client
            .post("/games/tetris/api/runs")
            .header(ContentType::JSON)
            .body(serde_json::json!({"csrf":csrf}).to_string())
            .dispatch()
            .into_json()
            .unwrap();
        let id = run["id"].as_str().unwrap().to_owned();
        diesel::update(runs::table.find(&id))
            .set(runs::created_at.eq(chrono::Utc::now().timestamp_millis() - 11_000))
            .execute(&mut conn)
            .unwrap();
        let response = client
            .post(format!("/games/tetris/api/runs/{id}/score"))
            .header(ContentType::JSON)
            .body(
                serde_json::json!({
                    "player_name":"Guest",
                    "score":1200,
                    "lines":4,
                    "level":2,
                    "csrf":csrf
                })
                .to_string(),
            )
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
        let score: serde_json::Value = response.into_json().unwrap();
        assert_eq!(score["player_name"], "Guest");
        assert_eq!(score["score"], 1200);
        let response = client
            .post(format!("/games/tetris/api/runs/{id}/score"))
            .header(ContentType::JSON)
            .body(
                serde_json::json!({
                    "player_name":"Another",
                    "score":5000,
                    "lines":10,
                    "level":2,
                    "csrf":csrf
                })
                .to_string(),
            )
            .dispatch();
        assert_eq!(response.status(), Status::Conflict);
        let scores: Vec<serde_json::Value> = client
            .get("/games/tetris/api/scores")
            .dispatch()
            .into_json()
            .unwrap();
        assert_eq!(scores.len(), 1);
        assert_eq!(scores[0]["score"], 1200);

        let second_run: serde_json::Value = client
            .post("/games/tetris/api/runs")
            .header(ContentType::JSON)
            .body(serde_json::json!({"csrf":csrf}).to_string())
            .dispatch()
            .into_json()
            .unwrap();
        let second_id = second_run["id"].as_str().unwrap();
        diesel::update(runs::table.find(second_id))
            .set(runs::created_at.eq(chrono::Utc::now().timestamp_millis() - 11_000))
            .execute(&mut conn)
            .unwrap();
        assert_eq!(
            client
                .post(format!("/games/tetris/api/runs/{second_id}/score"))
                .header(ContentType::JSON)
                .body(
                    serde_json::json!({
                        "player_name":"Guest two",
                        "score":5000,
                        "lines":10,
                        "level":2,
                        "csrf":csrf
                    })
                    .to_string(),
                )
                .dispatch()
                .status(),
            Status::Ok
        );
        let scores: Vec<serde_json::Value> = client
            .get("/games/tetris/api/scores")
            .dispatch()
            .into_json()
            .unwrap();
        assert_eq!(scores[0]["score"], 5000);
        assert_eq!(scores[1]["score"], 1200);
    }
}
