use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use diesel::prelude::*;
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome, Request};
use rocket::serde::json::Json;
use rocket::{State, post};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    db::DbPool,
    management::{self, Record, Section},
    models::User,
    pages::Error,
    schema::users,
};

const SESSION_LIFETIME: Duration = Duration::from_secs(3600);
const LOGIN_WINDOW: Duration = Duration::from_secs(300);
const MAX_FAILURES: u32 = 5;

struct Session {
    user_id: i32,
    version: i64,
    ip: IpAddr,
    expires: Instant,
}

#[derive(Default)]
struct AuthState {
    sessions: HashMap<String, Session>,
    attempts: HashMap<IpAddr, (Instant, u32)>,
}

pub struct RemoteManagement {
    allowed: HashSet<IpAddr>,
    proxies: HashSet<IpAddr>,
    auth: Mutex<AuthState>,
}

fn normalize(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(ip),
        _ => ip,
    }
}

impl RemoteManagement {
    pub fn from_env() -> Result<Self, Error> {
        fn setting(name: &str) -> Result<String, Error> {
            match std::env::var(name) {
                Ok(value) => Ok(value),
                Err(std::env::VarError::NotPresent) => Ok(String::new()),
                Err(error) => Err(format!("Invalid {name}: {error}").into()),
            }
        }
        Self::new(
            &setting("MANAGEMENT_ALLOWED_IPS")?,
            &setting("MANAGEMENT_TRUSTED_PROXIES")?,
        )
    }

    pub fn new(allowed: &str, proxies: &str) -> Result<Self, Error> {
        fn parse(value: &str) -> Result<HashSet<IpAddr>, Error> {
            value
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|s| !s.is_empty())
                .map(|s| {
                    s.parse::<IpAddr>().map(normalize).map_err(|_| {
                        format!("Invalid management IP address: {s} (use exact IPs, not CIDR)")
                            .into()
                    })
                })
                .collect()
        }
        Ok(Self {
            allowed: parse(allowed)?,
            proxies: parse(proxies)?,
            auth: Mutex::default(),
        })
    }

    fn client_ip(&self, request: &Request<'_>) -> Result<IpAddr, Status> {
        let peer = normalize(request.remote().ok_or(Status::Forbidden)?.ip());
        let ip = if self.proxies.contains(&peer) {
            let single_header = |name| {
                let mut values = request.headers().get(name);
                let value = values.next().ok_or(Status::Forbidden)?;
                if values.next().is_some() {
                    return Err(Status::Forbidden);
                }
                Ok(value)
            };
            if single_header("X-Forwarded-Proto")? != "https" {
                return Err(Status::Forbidden);
            }
            normalize(
                single_header("X-Real-IP")?
                    .parse()
                    .map_err(|_| Status::Forbidden)?,
            )
        } else {
            // This Rocket build has no direct TLS listener. Remote HTTPS must
            // terminate at an explicitly trusted proxy; direct HTTP is local only.
            if !peer.is_loopback() {
                return Err(Status::Forbidden);
            }
            peer
        };
        if !self.allowed.contains(&ip) {
            return Err(Status::Forbidden);
        }
        Ok(ip)
    }
}

pub fn routes() -> Vec<rocket::Route> {
    rocket::routes![login, logout, list, save, remove, details]
}

pub fn catchers() -> Vec<rocket::Catcher> {
    rocket::catchers![login_required]
}

#[rocket::catch(401)]
fn login_required() -> (Status, &'static str) {
    (
        Status::Unauthorized,
        "Admin login required; restart manage to sign in again.",
    )
}

pub struct Trusted(IpAddr);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Trusted {
    type Error = ();
    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, ()> {
        let Some(config) = request.rocket().state::<RemoteManagement>() else {
            eprintln!("Remote management configuration unavailable");
            return Outcome::Error((Status::InternalServerError, ()));
        };
        match config.client_ip(request) {
            Ok(ip) => Outcome::Success(Self(ip)),
            Err(status) => Outcome::Error((status, ())),
        }
    }
}

pub struct Admin {
    id: i32,
    version: i64,
    username: String,
    token: String,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Admin {
    type Error = ();
    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, ()> {
        let trusted = match request.guard::<Trusted>().await {
            Outcome::Success(trusted) => trusted,
            Outcome::Error(error) => return Outcome::Error(error),
            Outcome::Forward(status) => return Outcome::Error((status, ())),
        };
        let result = (|| -> Result<Self, ApiError> {
            let config = request
                .rocket()
                .state::<RemoteManagement>()
                .ok_or_else(|| internal("Missing configuration"))?;
            let pool = request
                .rocket()
                .state::<DbPool>()
                .ok_or_else(|| internal("Missing database"))?;
            let mut headers = request.headers().get("Authorization");
            let token = headers
                .next()
                .and_then(|h| h.strip_prefix("Bearer "))
                .filter(|t| !t.is_empty())
                .ok_or_else(unauthorized)?;
            if headers.next().is_some() {
                return Err(unauthorized());
            }
            let (id, version) = {
                let mut auth = config.auth.lock().map_err(internal)?;
                auth.sessions.retain(|_, s| s.expires > Instant::now());
                let session = auth
                    .sessions
                    .get(token)
                    .filter(|s| s.ip == trusted.0)
                    .ok_or_else(unauthorized)?;
                (session.user_id, session.version)
            };
            let mut conn = pool.get().map_err(internal)?;
            let user = active_admin(&mut conn, id, version)?;
            Ok(Self {
                id,
                version,
                username: user.username,
                token: token.into(),
            })
        })();
        match result {
            Ok(admin) => Outcome::Success(admin),
            Err((status, _)) => Outcome::Error((status, ())),
        }
    }
}

type ApiError = (Status, String);
type ApiResult<T> = Result<Json<T>, ApiError>;

fn unauthorized() -> ApiError {
    (
        Status::Unauthorized,
        "Admin login required; restart manage to sign in again.".into(),
    )
}
fn internal(error: impl std::fmt::Display) -> ApiError {
    eprintln!("Remote management failed: {error}");
    (
        Status::InternalServerError,
        "Remote management operation failed. See server logs.".into(),
    )
}
fn operation(error: Error) -> ApiError {
    eprintln!("Remote management operation rejected: {error}");
    (Status::UnprocessableEntity, error.to_string())
}

fn active_admin(conn: &mut SqliteConnection, id: i32, version: i64) -> Result<User, ApiError> {
    users::table
        .find(id)
        .first::<User>(conn)
        .optional()
        .map_err(internal)?
        .filter(|u| !u.disabled && u.role == "admin" && u.session_version == version)
        .ok_or_else(unauthorized)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Login {
    pub username: String,
    pub password: String,
}

#[derive(Deserialize, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub username: String,
}

#[post("/api/manage/login", format = "json", data = "<payload>")]
fn login(
    trusted: Trusted,
    payload: Json<Login>,
    pool: &State<DbPool>,
    config: &State<RemoteManagement>,
) -> ApiResult<LoginResponse> {
    let now = Instant::now();
    {
        let mut auth = config.auth.lock().map_err(internal)?;
        auth.attempts
            .retain(|_, (start, _)| now.duration_since(*start) < LOGIN_WINDOW);
        if auth.attempts.len() >= 4096 && !auth.attempts.contains_key(&trusted.0) {
            return Err((
                Status::TooManyRequests,
                "Too many login attempts. Try again later.".into(),
            ));
        }
        let attempt = auth.attempts.entry(trusted.0).or_insert((now, 0));
        if attempt.1 >= MAX_FAILURES {
            return Err((
                Status::TooManyRequests,
                "Too many login attempts. Try again in five minutes.".into(),
            ));
        }
        attempt.1 += 1;
    }
    if payload.username.len() > 400 || payload.password.is_empty() || payload.password.len() > 72 {
        return Err(unauthorized());
    }
    let mut conn = pool.get().map_err(internal)?;
    let user = users::table
        .filter(users::username.eq(&payload.username))
        .first::<User>(&mut conn)
        .optional()
        .map_err(internal)?;
    let Some(user) = user.filter(|u| !u.disabled && u.role == "admin") else {
        return Err(unauthorized());
    };
    if !bcrypt::verify(&payload.password, &user.password_hash).map_err(internal)? {
        return Err(unauthorized());
    }
    let mut auth = config.auth.lock().map_err(internal)?;
    auth.sessions.retain(|_, s| s.expires > now);
    if auth.sessions.len() >= 1024 {
        return Err((
            Status::ServiceUnavailable,
            "Too many active management sessions. Try again later.".into(),
        ));
    }
    let token = uuid::Uuid::new_v4().to_string();
    auth.sessions.insert(
        token.clone(),
        Session {
            user_id: user.id,
            version: user.session_version,
            ip: trusted.0,
            expires: now + SESSION_LIFETIME,
        },
    );
    auth.attempts.remove(&trusted.0);
    Ok(Json(LoginResponse {
        token,
        username: user.username,
    }))
}

#[post("/api/manage/logout")]
fn logout(admin: Admin, config: &State<RemoteManagement>) -> Result<Status, ApiError> {
    config
        .auth
        .lock()
        .map_err(internal)?
        .sessions
        .remove(&admin.token);
    Ok(Status::NoContent)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub section: Section,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Save {
    pub section: Section,
    pub original: Option<Record>,
    pub values: Value,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Selected {
    pub section: Section,
    pub record: Record,
}

#[post("/api/manage/list", format = "json", data = "<payload>")]
fn list(_admin: Admin, payload: Json<Selection>, pool: &State<DbPool>) -> ApiResult<Vec<Record>> {
    let mut conn = pool.get().map_err(internal)?;
    Ok(Json(
        management::list(&mut conn, payload.section).map_err(operation)?,
    ))
}

#[post("/api/manage/save", format = "json", data = "<payload>")]
fn save(admin: Admin, payload: Json<Save>, pool: &State<DbPool>) -> Result<Status, ApiError> {
    let mut payload = payload.into_inner();
    let mut conn = pool.get().map_err(internal)?;
    if payload.section == Section::Pages {
        let values = payload
            .values
            .as_object_mut()
            .ok_or((Status::UnprocessableEntity, "Expected form fields".into()))?;
        values.insert("editor".into(), Value::String(admin.username.clone()));
    }
    management::save_authorized(
        &mut conn,
        payload.section,
        payload.original.as_ref(),
        payload.values,
        |conn| {
            active_admin(conn, admin.id, admin.version)
                .map(|_| ())
                .map_err(|(_, message)| message.into())
        },
    )
    .map_err(operation)?;
    Ok(Status::NoContent)
}

#[post("/api/manage/remove", format = "json", data = "<payload>")]
fn remove(admin: Admin, payload: Json<Selected>, pool: &State<DbPool>) -> Result<Status, ApiError> {
    let mut conn = pool.get().map_err(internal)?;
    management::remove_authorized(
        &mut conn,
        payload.section,
        &payload.record,
        &admin.username,
        |conn| {
            active_admin(conn, admin.id, admin.version)
                .map(|_| ())
                .map_err(|(_, message)| message.into())
        },
    )
    .map_err(operation)?;
    Ok(Status::NoContent)
}

#[post("/api/manage/details", format = "json", data = "<payload>")]
fn details(_admin: Admin, payload: Json<Selected>, pool: &State<DbPool>) -> ApiResult<String> {
    let mut conn = pool.get().map_err(internal)?;
    let current = management::list(&mut conn, payload.section)
        .map_err(operation)?
        .into_iter()
        .find(|r| r.id == payload.record.id)
        .ok_or((Status::NotFound, "Record not found".into()))?;
    Ok(Json(
        management::details(&mut conn, payload.section, &current).map_err(operation)?,
    ))
}

pub fn no_cache() -> rocket::fairing::AdHoc {
    rocket::fairing::AdHoc::on_response("Private management API", |request, response| {
        Box::pin(async move {
            if request.uri().path().as_str().starts_with("/api/manage/") {
                response.set_raw_header("Cache-Control", "no-store");
                response.set_raw_header("Pragma", "no-cache");
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocket::http::{ContentType, Header};
    use rocket::local::blocking::Client;
    use serde_json::json;

    #[test]
    fn expired_sessions_and_login_windows_are_enforced() {
        let dir = tempfile::tempdir().unwrap();
        let pool = crate::db::establish_pool(dir.path().join("remote.db").to_str().unwrap());
        {
            let mut conn = pool.get().unwrap();
            crate::db::run_migrations(&mut conn).unwrap();
            diesel::insert_into(users::table)
                .values(crate::models::NewUser {
                    username: "admin",
                    password_hash: &bcrypt::hash("password", 4).unwrap(),
                    role: "admin",
                    created_at: 0,
                    email: None,
                })
                .execute(&mut conn)
                .unwrap();
        }
        let client = Client::tracked(
            rocket::build()
                .manage(pool)
                .manage(RemoteManagement::new("127.0.0.1", "").unwrap())
                .mount("/", routes()),
        )
        .unwrap();
        let token = client
            .post("/api/manage/login")
            .remote("127.0.0.1:1234".parse().unwrap())
            .header(ContentType::JSON)
            .body(json!({"username":"admin","password":"password"}).to_string())
            .dispatch()
            .into_json::<LoginResponse>()
            .unwrap()
            .token;
        {
            let config = client.rocket().state::<RemoteManagement>().unwrap();
            let mut auth = config.auth.lock().unwrap();
            let session = auth.sessions.get_mut(&token).unwrap();
            session.expires = Instant::now() - Duration::from_secs(1);
            auth.attempts.insert(
                "127.0.0.1".parse().unwrap(),
                (Instant::now() - LOGIN_WINDOW, MAX_FAILURES),
            );
        }
        assert_eq!(
            client
                .post("/api/manage/list")
                .remote("127.0.0.1:1234".parse().unwrap())
                .header(Header::new("Authorization", format!("Bearer {token}")))
                .header(ContentType::JSON)
                .body(json!({"section":"users"}).to_string())
                .dispatch()
                .status(),
            Status::Unauthorized
        );
        assert_eq!(
            client
                .post("/api/manage/login")
                .remote("127.0.0.1:1234".parse().unwrap())
                .header(ContentType::JSON)
                .body(json!({"username":"admin","password":"password"}).to_string())
                .dispatch()
                .status(),
            Status::Ok
        );
    }
}
