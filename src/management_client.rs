use std::time::Duration;

use reqwest::{Url, blocking::Client, redirect::Policy};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{
    db::DbPool,
    management::{self, Record, Section},
    pages::Error,
    remote_management as api,
};

pub fn remote_url(value: &str) -> Result<Url, Error> {
    let url = Url::parse(value)?;
    let host = url.host_str().unwrap_or("").trim_matches(['[', ']']);
    let local = host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    if url.scheme() != "https" && !(url.scheme() == "http" && local) {
        return Err("Remote management requires HTTPS (HTTP is allowed only on localhost).".into());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err("Use a site origin such as https://wasd.dk, without credentials, path, query, or fragment.".into());
    }
    Ok(url)
}

pub struct Remote {
    client: Client,
    origin: Url,
    token: String,
    pub username: String,
}

impl Remote {
    pub fn login(origin: Url, username: String, password: String) -> Result<Self, Error> {
        let origin = remote_url(origin.as_str())?;
        let builder = Client::builder()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30));
        let client = if origin.scheme() == "http" {
            builder.no_proxy()
        } else {
            builder
        }
        .build()?;
        let response = client
            .post(origin.join("api/manage/login")?)
            .json(&api::Login { username, password })
            .send()?;
        let login: api::LoginResponse = checked(response)?.json()?;
        Ok(Self {
            client,
            origin,
            token: login.token,
            username: login.username,
        })
    }

    fn request(
        &self,
        action: &str,
        body: &impl Serialize,
    ) -> Result<reqwest::blocking::Response, Error> {
        checked(
            self.client
                .post(self.origin.join(&format!("api/manage/{action}"))?)
                .bearer_auth(&self.token)
                .json(body)
                .send()?,
        )
    }

    fn json<T: DeserializeOwned>(&self, action: &str, body: &impl Serialize) -> Result<T, Error> {
        Ok(self.request(action, body)?.json()?)
    }

    fn write(&self, action: &str, body: &impl Serialize) -> Result<(), Error> {
        no_content(self.request(action, body)?)
    }

    pub fn logout(&mut self) -> Result<(), Error> {
        if !self.token.is_empty() {
            let token = std::mem::take(&mut self.token);
            let response = self
                .client
                .post(self.origin.join("api/manage/logout")?)
                .bearer_auth(token)
                .timeout(Duration::from_secs(5))
                .send()?;
            if response.status().as_u16() != 401 {
                no_content(checked(response)?)?;
            }
        }
        Ok(())
    }
}

fn no_content(response: reqwest::blocking::Response) -> Result<(), Error> {
    if response.status() != reqwest::StatusCode::NO_CONTENT {
        return Err(format!("Unexpected remote write response: HTTP {} (expected 204). Check the API deployment before retrying.", response.status()).into());
    }
    Ok(())
}

impl Drop for Remote {
    fn drop(&mut self) {
        if let Err(error) = self.logout() {
            eprintln!(
                "Remote logout failed: {error}. The session expires automatically after one hour."
            );
        }
    }
}

fn checked(response: reqwest::blocking::Response) -> Result<reqwest::blocking::Response, Error> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let message = match status.as_u16() {
        401 => "Admin session expired or login failed. Restart manage to sign in again.".into(),
        403 => "Remote management denied: check MANAGEMENT_ALLOWED_IPS and trusted proxy/TLS configuration.".into(),
        429 => "Too many login attempts. Try again in five minutes.".into(),
        300..=399 => "Unexpected redirect refused; check the remote HTTPS origin.".into(),
        _ => response.text()?,
    };
    Err(format!("Remote management HTTP {status}: {message}").into())
}

pub enum Store {
    Local(DbPool),
    Remote(Remote),
}

impl From<DbPool> for Store {
    fn from(pool: DbPool) -> Self {
        Self::Local(pool)
    }
}

impl Store {
    pub fn list(&self, section: Section) -> Result<Vec<Record>, Error> {
        match self {
            Self::Local(pool) => management::list(&mut *pool.get()?, section),
            Self::Remote(remote) => remote.json("list", &api::Selection { section }),
        }
    }

    pub fn save(
        &self,
        section: Section,
        original: Option<&Record>,
        values: Value,
    ) -> Result<(), Error> {
        match self {
            Self::Local(pool) => management::save(&mut *pool.get()?, section, original, values),
            Self::Remote(remote) => {
                remote.write(
                    "save",
                    &api::Save {
                        section,
                        original: original.cloned(),
                        values,
                    },
                )?;
                Ok(())
            }
        }
    }

    pub fn remove(
        &self,
        section: Section,
        record: &Record,
        attribution: &str,
    ) -> Result<(), Error> {
        match self {
            Self::Local(pool) => {
                management::remove(&mut *pool.get()?, section, record, attribution)
            }
            Self::Remote(remote) => {
                remote.write(
                    "remove",
                    &api::Selected {
                        section,
                        record: record.clone(),
                    },
                )?;
                Ok(())
            }
        }
    }

    pub fn details(&self, section: Section, record: &Record) -> Result<String, Error> {
        match self {
            Self::Local(pool) => management::details(&mut *pool.get()?, section, record),
            Self::Remote(remote) => remote.json(
                "details",
                &api::Selected {
                    section,
                    record: record.clone(),
                },
            ),
        }
    }

    pub fn logout(&mut self) -> Result<(), Error> {
        if let Self::Remote(remote) = self {
            remote.logout()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn response_server(response: &'static str) -> (Url, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = remote_url(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut input = Vec::new();
            loop {
                let mut chunk = [0; 8192];
                let count = stream.read(&mut chunk).unwrap();
                assert!(count > 0);
                input.extend_from_slice(&chunk[..count]);
                if let Some(end) = input.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&input[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .map(|value| value.parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if input.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            stream.write_all(response.as_bytes()).unwrap();
        });
        (origin, thread)
    }

    #[test]
    fn refuses_redirects_without_resending_credentials() {
        let (origin, thread) = response_server(
            "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/stolen\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        let result = Remote::login(origin, "admin".into(), "password".into());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("redirect refused")
        );
        thread.join().unwrap();
    }

    #[test]
    fn does_not_treat_html_success_as_a_saved_write() {
        let (origin, thread) = response_server(
            "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nhtml",
        );
        let mut remote = Remote {
            client: Client::builder().no_proxy().build().unwrap(),
            origin,
            token: "test-session".into(),
            username: "admin".into(),
        };
        let result = remote.write("save", &serde_json::json!({}));
        remote.token.clear();
        assert!(result.unwrap_err().to_string().contains("expected 204"));
        thread.join().unwrap();
    }

    #[test]
    fn validates_secure_origins_without_credential_redirects() {
        for value in [
            "https://example.org",
            "http://localhost:5010",
            "http://127.0.0.1:5010",
            "http://[::1]:5010",
        ] {
            assert!(remote_url(value).is_ok(), "{value}");
        }
        for value in [
            "http://example.org",
            "ftp://localhost",
            "https://user:password@example.org",
            "https://example.org/admin",
            "https://example.org/?token=x",
            "https://example.org/#token",
            "http://localhost.example.org",
            "http://192.168.1.1",
        ] {
            assert!(remote_url(value).is_err(), "{value}");
        }
    }
}
