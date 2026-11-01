use std::{
    collections::HashMap,
    net::IpAddr,
    sync::Mutex,
    time::{Duration, Instant},
};

use diesel::prelude::*;
use rocket::response::{self, Responder};
use rocket::{
    State, get,
    http::Status,
    post,
    request::{FromRequest, Outcome, Request},
    serde::json::Json,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    db::DbPool,
    models::User,
    schema::{player_sessions as sessions, users},
};

type ApiError = (Status, String);
type ApiResult<T> = Result<PrivateJson<T>, ApiError>;

struct PrivateJson<T>(T);

impl<'r, T: Serialize> Responder<'r, 'static> for PrivateJson<T> {
    fn respond_to(self, request: &'r Request<'_>) -> response::Result<'static> {
        let mut response = Json(self.0).respond_to(request)?;
        response.set_raw_header("Cache-Control", "private, no-store");
        response.set_raw_header("Vary", "Authorization");
        Ok(response)
    }
}

#[derive(Default)]
pub struct PlayerLoginLimits(Mutex<HashMap<(IpAddr, String), (Instant, u32)>>);

pub fn routes() -> Vec<rocket::Route> {
    rocket::routes![login, me, logout]
}

pub fn catchers() -> Vec<rocket::Catcher> {
    rocket::catchers![login_required]
}

#[rocket::catch(401)]
fn login_required() -> (Status, &'static str) {
    (
        Status::Unauthorized,
        "Player login required or session expired.",
    )
}

fn internal(error: impl std::fmt::Display) -> ApiError {
    eprintln!("Player authentication failed: {error}");
    (
        Status::InternalServerError,
        "Player authentication is unavailable. Please try again.".into(),
    )
}

fn unauthorized() -> ApiError {
    (
        Status::Unauthorized,
        "Player login required or session expired.".into(),
    )
}

fn hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

pub struct Player {
    pub user: User,
    token_hash: String,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Player {
    type Error = ();
    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, ()> {
        let result = (|| {
            let pool = request
                .rocket()
                .state::<DbPool>()
                .ok_or_else(|| internal("Missing database"))?;
            let mut headers = request.headers().get("Authorization");
            let token = headers
                .next()
                .and_then(|h| h.strip_prefix("Bearer "))
                .filter(|t| t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit()))
                .ok_or_else(unauthorized)?;
            if headers.next().is_some() {
                return Err(unauthorized());
            }
            let token_hash = hash(token);
            let mut conn = pool.get().map_err(internal)?;
            let session = sessions::table
                .find(&token_hash)
                .select((
                    sessions::user_id,
                    sessions::session_version,
                    sessions::expires_at,
                ))
                .first::<(i32, i64, i64)>(&mut conn)
                .optional()
                .map_err(internal)?
                .filter(|s| s.2 > chrono::Utc::now().timestamp())
                .ok_or_else(unauthorized)?;
            let user = users::table
                .find(session.0)
                .first::<User>(&mut conn)
                .optional()
                .map_err(internal)?
                .filter(|u| !u.disabled && u.session_version == session.1)
                .ok_or_else(unauthorized)?;
            Ok(Self { user, token_hash })
        })();
        match result {
            Ok(player) => Outcome::Success(player),
            Err((status, _)) => Outcome::Error((status, ())),
        }
    }
}

struct LoginPeer(IpAddr);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for LoginPeer {
    type Error = ();
    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, ()> {
        match request.remote() {
            Some(remote) => Outcome::Success(Self(remote.ip())),
            None => Outcome::Error((Status::BadRequest, ())),
        }
    }
}

#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
    #[serde(default)]
    remember: bool,
}

#[derive(Serialize)]
struct LoginResponse {
    token: String,
    display_name: String,
    expires_at: i64,
}

#[derive(Serialize)]
struct Profile {
    display_name: String,
}

#[post("/api/player/login", format = "json", data = "<request>")]
fn login(
    pool: &State<DbPool>,
    limits: &State<PlayerLoginLimits>,
    peer: LoginPeer,
    request: Json<LoginRequest>,
) -> ApiResult<LoginResponse> {
    let username = request.username.trim();
    if username.is_empty() || username.len() > 128 || request.password.len() > 72 {
        return Err((Status::Unauthorized, "Invalid username or password.".into()));
    }
    {
        let mut attempts = limits.0.lock().map_err(internal)?;
        attempts.retain(|_, (start, _)| start.elapsed() < Duration::from_secs(300));
        if attempts.len() >= 10_000 {
            return Err((
                Status::TooManyRequests,
                "Too many login attempts. Try again later.".into(),
            ));
        }
        let entry = attempts
            .entry((peer.0, username.to_lowercase()))
            .or_insert((Instant::now(), 0));
        if entry.1 >= 5 {
            return Err((
                Status::TooManyRequests,
                "Too many login attempts. Try again in five minutes.".into(),
            ));
        }
        entry.1 += 1;
    }
    let mut conn = pool.get().map_err(internal)?;
    let user = users::table
        .filter(users::username.eq(username))
        .first::<User>(&mut conn)
        .optional()
        .map_err(internal)?;
    // Use a valid dummy hash too, so unknown usernames still incur bcrypt work.
    const DUMMY_HASH: &str = "$2b$12$R9h/cIPz0gi.URNNX3kh2OPST9/PgBkqquzi.Ss7KIUgO2t0jWMUW";
    let valid = bcrypt::verify(
        &request.password,
        user.as_ref()
            .map_or(DUMMY_HASH, |u| u.password_hash.as_str()),
    )
    .map_err(internal)?;
    let user = user
        .filter(|u| valid && !u.disabled)
        .ok_or((Status::Unauthorized, "Invalid username or password.".into()))?;
    let now = chrono::Utc::now().timestamp();
    let expires_at = now
        + if request.remember {
            30 * 86_400
        } else {
            12 * 3600
        };
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    diesel::delete(sessions::table.filter(sessions::expires_at.le(now)))
        .execute(&mut conn)
        .map_err(internal)?;
    diesel::insert_into(sessions::table)
        .values((
            sessions::token_hash.eq(hash(&token)),
            sessions::user_id.eq(user.id),
            sessions::session_version.eq(user.session_version),
            sessions::expires_at.eq(expires_at),
        ))
        .execute(&mut conn)
        .map_err(internal)?;
    Ok(PrivateJson(LoginResponse {
        token,
        display_name: user.display_name().to_owned(),
        expires_at,
    }))
}

#[get("/api/player/me")]
fn me(player: Player) -> PrivateJson<Profile> {
    PrivateJson(Profile {
        display_name: player.user.display_name().to_owned(),
    })
}

#[post("/api/player/logout")]
fn logout(pool: &State<DbPool>, player: Player) -> ApiResult<bool> {
    let mut conn = pool.get().map_err(internal)?;
    diesel::delete(sessions::table.find(player.token_hash))
        .execute(&mut conn)
        .map_err(internal)?;
    Ok(PrivateJson(true))
}
