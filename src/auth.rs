use diesel::prelude::*;
use rocket::fairing::AdHoc;
use rocket::http::{Cookie, CookieJar};

use crate::{db::DbPool, models::User, pages::Error, schema::users};

pub fn cookie(jar: &CookieJar<'_>, name: &str) -> Option<Cookie<'static>> {
    // get_pending alone also accepts unsigned originals; authenticate before
    // observing the session fairing's pending removals and replacements.
    jar.get_private(name)?;
    jar.get_pending(name)
}

pub fn session_user(jar: &CookieJar<'_>, pool: &DbPool) -> Result<Option<User>, Error> {
    let expiry = jar
        .get_private("session_expires")
        .and_then(|c| c.value().parse::<i64>().ok());
    let id = jar
        .get_private("user_id")
        .and_then(|c| c.value().parse::<i32>().ok());
    let Some(id) = id.filter(|_| expiry.is_some_and(|ts| ts > chrono::Utc::now().timestamp()))
    else {
        return Ok(None);
    };
    let version = match jar.get_private("session_version") {
        Some(cookie) => match cookie.value().parse::<i64>() {
            Ok(version) => version,
            Err(_) => return Ok(None),
        },
        None => 0,
    };
    let mut conn = pool.get()?;
    Ok(users::table
        .find(id)
        .first::<User>(&mut conn)
        .optional()?
        .filter(|user| !user.disabled && user.session_version == version))
}

pub fn sessions() -> AdHoc {
    AdHoc::on_request("Validate current account and session", |request, _| {
        Box::pin(async move {
            let jar = request.cookies();
            let names = [
                "user_id",
                "username",
                "display_name",
                "user_role",
                "session_expires",
                "session_version",
            ];
            if names.iter().all(|name| jar.get_private(name).is_none()) {
                return;
            }
            let result = match request.rocket().state::<DbPool>() {
                Some(pool) => session_user(jar, pool),
                None => Err("Session database pool unavailable".into()),
            };
            match result {
                Ok(Some(user)) => {
                    let display_name = user.display_name().to_owned();
                    for (name, value) in [("username", user.username), ("display_name", display_name), ("user_role", user.role)] {
                        if jar.get_private(name).is_none_or(|c| c.value() != value) {
                            jar.add_private(Cookie::new(name, value));
                        }
                    }
                }
                other => {
                    if let Err(error) = other {
                        eprintln!("Session validation failed: {error}");
                    }
                    for name in names {
                        jar.remove_private(Cookie::new(name, ""));
                    }
                }
            }
        })
    })
}
