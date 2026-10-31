use diesel::prelude::*;
use rocket::http::{ContentType, Header, Status};
use rocket::local::blocking::Client;
use serde_json::{Value, json};
use wasd5::{
    db,
    management::{self, Record, Section},
    models::NewUser,
    pages, remote_management as api, schema,
};

fn pool(dir: &tempfile::TempDir) -> db::DbPool {
    let pool = db::establish_pool(dir.path().join("remote.db").to_str().unwrap());
    let mut conn = pool.get().unwrap();
    db::run_migrations(&mut conn).unwrap();
    for (name, role) in [
        ("admin", "admin"),
        ("other-admin", "admin"),
        ("member", "member"),
    ] {
        diesel::insert_into(schema::users::table)
            .values(NewUser {
                username: name,
                role,
                password_hash: &bcrypt::hash("password", 4).unwrap(),
                created_at: 0,
                email: None,
            })
            .execute(&mut conn)
            .unwrap();
    }
    pool.clone()
}

fn app(pool: db::DbPool, allowed: &str, proxies: &str) -> rocket::Rocket<rocket::Build> {
    rocket::custom(rocket::Config {
        log_level: rocket::config::LogLevel::Off,
        ..rocket::Config::debug_default()
    })
    .manage(pool)
    .manage(api::RemoteManagement::new(allowed, proxies).unwrap())
    .mount("/", api::routes())
    .register("/api/manage", api::catchers())
    .attach(api::no_cache())
}

fn setup(allowed: &str, proxies: &str) -> (Client, tempfile::TempDir, db::DbPool) {
    let dir = tempfile::tempdir().unwrap();
    let pool = pool(&dir);
    (
        Client::tracked(app(pool.clone(), allowed, proxies)).unwrap(),
        dir,
        pool,
    )
}

fn post<'a>(
    client: &'a Client,
    action: &str,
    token: Option<&str>,
    body: Value,
) -> rocket::local::blocking::LocalResponse<'a> {
    let mut request = client
        .post(format!("/api/manage/{action}"))
        .remote("127.0.0.1:12345".parse().unwrap())
        .header(ContentType::JSON)
        .body(body.to_string());
    if let Some(token) = token {
        request = request.header(Header::new("Authorization", format!("Bearer {token}")));
    }
    request.dispatch()
}

fn login(client: &Client) -> String {
    let response = post(
        client,
        "login",
        None,
        json!({"username":"admin","password":"password"}),
    );
    assert_eq!(response.status(), Status::Ok);
    assert_eq!(
        response.headers().get_one("Cache-Control"),
        Some("no-store")
    );
    response.into_json::<api::LoginResponse>().unwrap().token
}

fn list(client: &Client, token: &str, section: Section) -> Vec<Record> {
    let response = post(client, "list", Some(token), json!({"section":section}));
    assert_eq!(response.status(), Status::Ok);
    response.into_json().unwrap()
}

#[test]
fn deny_by_default_and_never_trust_spoofed_ip_headers() {
    let (client, _dir, _) = setup("", "");
    let response = post(
        &client,
        "login",
        None,
        json!({"username":"admin","password":"password"}),
    );
    assert_eq!(response.status(), Status::Forbidden);
    assert_eq!(
        response.headers().get_one("Cache-Control"),
        Some("no-store")
    );
    let (client, _dir, _) = setup("203.0.113.1", "");
    for peer in ["127.0.0.1:12345", "198.51.100.1:12345"] {
        assert_eq!(
            client
                .post("/api/manage/login")
                .remote(peer.parse().unwrap())
                .header(Header::new("X-Real-IP", "203.0.113.1"))
                .header(Header::new("X-Forwarded-Proto", "https"))
                .header(ContentType::JSON)
                .body(json!({"username":"admin","password":"password"}).to_string())
                .dispatch()
                .status(),
            Status::Forbidden
        );
    }
    assert!(api::RemoteManagement::new("not-an-ip", "").is_err());
    assert!(api::RemoteManagement::new("127.0.0.0/8", "").is_err());
}

#[test]
fn trusted_proxy_requires_single_https_and_allowlisted_client_ip() {
    let (client, _dir, _) = setup("203.0.113.1", "127.0.0.1");
    for (ip, proto, expected) in [
        ("203.0.113.1", "https", Status::Ok),
        ("203.0.113.2", "https", Status::Forbidden),
        ("203.0.113.1", "http", Status::Forbidden),
        ("not-an-ip", "https", Status::Forbidden),
        ("203.0.113.1, 203.0.113.2", "https", Status::Forbidden),
    ] {
        assert_eq!(
            client
                .post("/api/manage/login")
                .remote("127.0.0.1:12345".parse().unwrap())
                .header(Header::new("X-Real-IP", ip))
                .header(Header::new("X-Forwarded-Proto", proto))
                .header(ContentType::JSON)
                .body(json!({"username":"admin","password":"password"}).to_string())
                .dispatch()
                .status(),
            expected
        );
    }
    for name in ["X-Real-IP", "X-Forwarded-Proto"] {
        assert_eq!(
            client
                .post("/api/manage/login")
                .remote("127.0.0.1:12345".parse().unwrap())
                .header(Header::new("X-Real-IP", "203.0.113.1"))
                .header(Header::new("X-Forwarded-Proto", "https"))
                .header(Header::new(
                    name,
                    if name == "X-Real-IP" {
                        "203.0.113.1"
                    } else {
                        "https"
                    }
                ))
                .header(ContentType::JSON)
                .body(json!({"username":"admin","password":"password"}).to_string())
                .dispatch()
                .status(),
            Status::Forbidden
        );
    }
}

#[test]
fn admin_login_is_required_on_every_endpoint_and_is_rate_limited() {
    let (client, _dir, _) = setup("127.0.0.1", "");
    for action in ["list", "save", "remove", "details", "logout"] {
        assert_eq!(
            post(&client, action, None, json!({})).status(),
            Status::Unauthorized,
            "{action}"
        );
        assert_eq!(
            post(&client, action, Some("forged-token"), json!({})).status(),
            Status::Unauthorized,
            "{action}"
        );
    }
    for (username, password) in [
        ("member", "password"),
        ("admin", "wrong"),
        ("missing", "password"),
        ("admin", ""),
        ("admin", "wrong"),
    ] {
        assert_eq!(
            post(
                &client,
                "login",
                None,
                json!({"username":username,"password":password})
            )
            .status(),
            Status::Unauthorized
        );
    }
    assert_eq!(
        post(
            &client,
            "login",
            None,
            json!({"username":"admin","password":"password"})
        )
        .status(),
        Status::TooManyRequests
    );
}

#[test]
fn all_sections_use_the_same_crud_and_server_page_attribution() {
    let (client, _dir, pool) = setup("127.0.0.1", "");
    let token = login(&client);
    for section in Section::ALL {
        let rows = list(&client, &token, section);
        assert!(
            !serde_json::to_string(&rows)
                .unwrap()
                .contains("password_hash")
        );
    }
    for (section, changes) in [
        (
            Section::Users,
            json!({"username":"new-user","password":"new-password"}),
        ),
        (
            Section::Pages,
            json!({"slug":"remote/page","editor":"Spoofed","content":"# Remote"}),
        ),
        (Section::Appointments, json!({"title":"Remote appointment"})),
        (Section::People, json!({"name":"Remote person"})),
        (Section::AllowedIps, json!({"ip_address":"192.0.2.1"})),
        (Section::Timers, json!({"title":"Remote timer"})),
        (Section::Albums, json!({"title":"Remote album"})),
        (Section::Packages, json!({"name":"Remote parcel"})),
        (Section::Creatine, json!({"amount_grams":5})),
    ] {
        let before = list(&client, &token, section);
        let mut values = management::defaults(section, "Spoofed");
        for (key, value) in changes.as_object().unwrap() {
            values[key] = value.clone();
        }
        assert_eq!(
            post(
                &client,
                "save",
                Some(&token),
                json!({"section":section,"original":null,"values":values})
            )
            .status(),
            Status::NoContent,
            "{section:?}"
        );
        let record = list(&client, &token, section)
            .into_iter()
            .find(|r| before.iter().all(|b| b.id != r.id))
            .unwrap();
        let details = post(
            &client,
            "details",
            Some(&token),
            json!({"section":section,"record":record}),
        );
        assert_eq!(details.status(), Status::Ok);
        let details: String = details.into_json().unwrap();
        assert!(!details.contains("password_hash"));
        if section == Section::Pages {
            let page = pages::by_id(&mut pool.get().unwrap(), record.id).unwrap();
            assert_eq!(page.creator, "admin");
            let mut values = record.values.clone();
            values["content"] = json!("# Changed");
            values["editor"] = json!("Spoofed");
            assert_eq!(
                post(
                    &client,
                    "save",
                    Some(&token),
                    json!({"section":section,"original":record,"values":values})
                )
                .status(),
                Status::NoContent
            );
            assert_eq!(
                post(
                    &client,
                    "save",
                    Some(&token),
                    json!({"section":section,"original":record,"values":values})
                )
                .status(),
                Status::UnprocessableEntity
            );
            let current = list(&client, &token, section)
                .into_iter()
                .find(|r| r.id == record.id)
                .unwrap();
            assert_eq!(
                post(
                    &client,
                    "remove",
                    Some(&token),
                    json!({"section":section,"record":current})
                )
                .status(),
                Status::NoContent
            );
            assert!(
                pages::history(&mut pool.get().unwrap(), record.id)
                    .unwrap()
                    .iter()
                    .all(|r| r.editor == "admin")
            );
        } else {
            assert_eq!(
                post(
                    &client,
                    "save",
                    Some(&token),
                    json!({"section":section,"original":record,"values":record.values})
                )
                .status(),
                Status::NoContent
            );
            let current = list(&client, &token, section)
                .into_iter()
                .find(|r| r.id == record.id)
                .unwrap();
            assert_eq!(
                post(
                    &client,
                    "remove",
                    Some(&token),
                    json!({"section":section,"record":current})
                )
                .status(),
                Status::NoContent
            );
        }
    }
    let album = list(&client, &token, Section::Albums).remove(0);
    diesel::insert_into(schema::comic_strips::table)
        .values((
            schema::comic_strips::album_id.eq(album.id),
            schema::comic_strips::title.eq("Strip"),
            schema::comic_strips::image_filename.eq("keep.png"),
            schema::comic_strips::display_order.eq(0),
            schema::comic_strips::hidden.eq(false),
            schema::comic_strips::created_at.eq(0),
            schema::comic_strips::updated_at.eq(0),
        ))
        .execute(&mut pool.get().unwrap())
        .unwrap();
    let strip = list(&client, &token, Section::Strips).remove(0);
    let mut values = strip.values.clone();
    values["title"] = json!("Edited strip");
    values["display_order"] = json!(2);
    assert_eq!(
        post(
            &client,
            "save",
            Some(&token),
            json!({"section":"strips","original":strip,"values":values})
        )
        .status(),
        Status::NoContent
    );
    let strip = list(&client, &token, Section::Strips).remove(0);
    assert_eq!(
        post(
            &client,
            "remove",
            Some(&token),
            json!({"section":"strips","record":strip})
        )
        .status(),
        Status::NoContent
    );
    assert_eq!(
        schema::comic_strips::table
            .select(schema::comic_strips::image_filename)
            .first::<String>(&mut pool.get().unwrap())
            .unwrap(),
        "keep.png"
    );
}

struct TestServer {
    address: std::net::SocketAddr,
    shutdown: rocket::Shutdown,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.shutdown.clone().notify();
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

fn server(pool: db::DbPool) -> TestServer {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = probe.local_addr().unwrap();
    drop(probe);
    let (sender, receiver) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        rocket::tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async {
                let rocket = app(pool, "127.0.0.1", "")
                    .configure(rocket::Config {
                        port: address.port(),
                        address: address.ip(),
                        log_level: rocket::config::LogLevel::Off,
                        ..rocket::Config::debug_default()
                    })
                    .ignite()
                    .await
                    .unwrap();
                sender.send(rocket.shutdown()).unwrap();
                rocket.launch().await.unwrap();
            });
    });
    let server = TestServer {
        address,
        shutdown: receiver
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap(),
        thread: Some(thread),
    };
    for _ in 0..100 {
        if std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_millis(100))
            .is_ok()
        {
            return server;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("Remote API did not become ready");
}

#[test]
fn remote_store_round_trip_over_real_http_preserves_local_editor_workflow() {
    use wasd5::management_client::{self, Remote, Store};
    let dir = tempfile::tempdir().unwrap();
    let pool = pool(&dir);
    let server = server(pool.clone());
    diesel::update(wasd5::schema::users::table.filter(wasd5::schema::users::username.eq("admin")))
        .set(wasd5::schema::users::display_name.eq(Some("Admin Display")))
        .execute(&mut pool.get().unwrap()).unwrap();
    let origin = management_client::remote_url(&format!("http://{}", server.address)).unwrap();
    let remote = Remote::login(origin, "admin".into(), "password".into()).unwrap();
    assert_eq!(remote.username, "admin");
    assert_eq!(remote.display_name, "Admin Display");
    let mut store = Store::Remote(remote);
    for section in Section::ALL {
        store.list(section).unwrap();
    }
    let mut values = management::defaults(Section::Pages, "Spoofed");
    values["slug"] = json!("network/page");
    values["content"] = json!("# Written in a local editor");
    store.save(Section::Pages, None, values).unwrap();
    let record = store.list(Section::Pages).unwrap().remove(0);
    let mut values = record.values.clone();
    values["content"] = json!("# Edited through HTTP");
    store
        .save(Section::Pages, Some(&record), values.clone())
        .unwrap();
    assert!(store.save(Section::Pages, Some(&record), values).is_err());
    let record = store.list(Section::Pages).unwrap().remove(0);
    assert!(
        store
            .details(Section::Pages, &record)
            .unwrap()
            .contains("# Written in a local editor")
    );
    store.remove(Section::Pages, &record, "Spoofed").unwrap();
    assert!(
        pages::by_id(&mut pool.get().unwrap(), record.id)
            .unwrap()
            .hidden
    );
    assert!(
        pages::history(&mut pool.get().unwrap(), record.id)
            .unwrap()
            .iter()
            .all(|r| r.editor == "Admin Display")
    );
    store.logout().unwrap();
    assert!(store.list(Section::Users).is_err());
}

#[test]
fn session_bound_to_ip_revoked_by_account_changes_and_logout() {
    let (client, _dir, pool) = setup("127.0.0.1,127.0.0.2", "");
    let token = login(&client);
    assert_eq!(
        client
            .post("/api/manage/list")
            .remote("127.0.0.2:12345".parse().unwrap())
            .header(Header::new("Authorization", format!("Bearer {token}")))
            .header(ContentType::JSON)
            .body(json!({"section":"users"}).to_string())
            .dispatch()
            .status(),
        Status::Unauthorized
    );
    for change in ["password", "role", "disabled"] {
        let token = login(&client);
        let admin = list(&client, &token, Section::Users)
            .into_iter()
            .find(|r| r.values["username"] == "admin")
            .unwrap();
        let mut values = admin.values.clone();
        values[change] = match change {
            "password" => json!("password"),
            "role" => json!("member"),
            _ => json!(true),
        };
        assert_eq!(
            post(
                &client,
                "save",
                Some(&token),
                json!({"section":"users","original":admin,"values":values})
            )
            .status(),
            Status::NoContent
        );
        assert_eq!(
            post(&client, "list", Some(&token), json!({"section":"users"})).status(),
            Status::Unauthorized
        );
        if change != "password" {
            assert_eq!(
                post(
                    &client,
                    "login",
                    None,
                    json!({"username":"admin","password":"password"})
                )
                .status(),
                Status::Unauthorized
            );
            let admin = management::list(&mut pool.get().unwrap(), Section::Users)
                .unwrap()
                .into_iter()
                .find(|r| r.values["username"] == "admin")
                .unwrap();
            let mut values = admin.values.clone();
            values["disabled"] = json!(false);
            values["role"] = json!("admin");
            management::save(
                &mut pool.get().unwrap(),
                Section::Users,
                Some(&admin),
                values,
            )
            .unwrap();
            assert_eq!(
                post(&client, "list", Some(&token), json!({"section":"users"})).status(),
                Status::Unauthorized
            );
        }
    }
    let token = login(&client);
    assert_eq!(
        post(&client, "logout", Some(&token), json!({})).status(),
        Status::NoContent
    );
    assert_eq!(
        post(&client, "list", Some(&token), json!({"section":"users"})).status(),
        Status::Unauthorized
    );
}
