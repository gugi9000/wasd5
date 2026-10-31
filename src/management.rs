use chrono::{NaiveDate, NaiveTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{models, pages, schema::*, workouts::Workout};
pub type Result<T> = std::result::Result<T, pages::Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    Users,
    Pages,
    Appointments,
    People,
    AllowedIps,
    Timers,
    Albums,
    Strips,
    Packages,
    Creatine,
}

impl Section {
    pub const ALL: [Self; 10] = [
        Self::Users,
        Self::Pages,
        Self::Appointments,
        Self::People,
        Self::AllowedIps,
        Self::Timers,
        Self::Albums,
        Self::Strips,
        Self::Packages,
        Self::Creatine,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Users => "Users",
            Self::Pages => "Pages",
            Self::Appointments => "Calendar: appointments",
            Self::People => "Calendar: people",
            Self::AllowedIps => "Calendar: allowed IPs",
            Self::Timers => "Fitness timers",
            Self::Albums => "Comics: albums",
            Self::Strips => "Comics: strips",
            Self::Packages => "Packages",
            Self::Creatine => "Creatine logs",
        }
    }

    pub fn delete_label(self) -> &'static str {
        match self {
            Self::Users => "Disable account (preserve all content)",
            Self::Pages | Self::Albums | Self::Strips => "Hide from public view",
            _ => "Permanently delete",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    Password,
    Integer,
    Decimal,
    Boolean,
    Markdown,
    Json,
}

#[derive(Clone, Copy)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: Kind,
}

impl Section {
    pub fn fields(self) -> Vec<Field> {
        use Kind::*;
        let fields = match self {
            Self::Users => vec![
                ("username", "Username", Text),
                ("display_name", "Display Name (blank uses username)", Text),
                ("role", "Role: admin or member", Text),
                ("email", "Email (optional)", Text),
                ("password", "Password (blank keeps existing)", Password),
                ("disabled", "Disabled", Boolean),
                ("creatine_reminder", "Creatine email reminders", Boolean),
            ],
            Self::Pages => vec![
                ("slug", "Slug (cannot rename existing pages)", Text),
                ("editor", "Editor attribution (required)", Text),
                ("hidden", "Hidden draft", Boolean),
                ("content", "Markdown (Enter opens system editor)", Markdown),
            ],
            Self::Appointments => vec![
                ("person_id", "Person ID (see Calendar: people)", Integer),
                ("title", "Title", Text),
                ("date", "Date YYYY-MM-DD", Text),
                ("start_time", "Start HH:MM (optional)", Text),
                ("end_time", "End HH:MM (optional)", Text),
            ],
            Self::People => vec![
                ("name", "Name", Text),
                ("display_order", "Display order (0+)", Integer),
            ],
            Self::AllowedIps => vec![("ip_address", "Allowed IP address (not CIDR)", Text)],
            Self::Timers => vec![
                ("owner_id", "Owner user ID", Integer),
                ("title", "Title", Text),
                ("description", "Description", Text),
                ("workout", "Workout JSON (Enter opens editor)", Json),
            ],
            Self::Albums => vec![
                ("owner_id", "Owner user ID", Integer),
                ("title", "Title", Text),
                ("subtext", "Caption", Text),
                ("hidden", "Hidden draft", Boolean),
            ],
            Self::Strips => vec![
                ("album_id", "Album ID", Integer),
                ("title", "Title", Text),
                ("subtext", "Caption", Text),
                ("hidden", "Hidden draft", Boolean),
                ("display_order", "Display order (0+)", Integer),
            ],
            Self::Packages => vec![
                ("user_id", "User ID", Integer),
                ("name", "Package name", Text),
                ("tracking_id", "Tracking ID (optional)", Text),
                ("ordered_date", "Ordered date YYYY-MM-DD", Text),
                (
                    "received_date",
                    "Received date YYYY-MM-DD (blank = pending)",
                    Text,
                ),
            ],
            Self::Creatine => vec![
                ("user_id", "User ID", Integer),
                ("date", "Date YYYY-MM-DD", Text),
                ("amount_grams", "Amount in grams (>0)", Decimal),
            ],
        };
        fields
            .into_iter()
            .map(|(key, label, kind)| Field { key, label, kind })
            .collect()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub id: i32,
    pub summary: String,
    pub values: Value,
    pub revision: Option<i32>,
}

fn row(id: i32, summary: String, values: Value) -> Record {
    Record {
        id,
        summary,
        values,
        revision: None,
    }
}

fn state(hidden: bool) -> &'static str {
    if hidden { "hidden" } else { "public" }
}

fn date_string(timestamp: i64) -> Result<String> {
    Ok(chrono::DateTime::from_timestamp(timestamp, 0)
        .ok_or("Invalid stored timestamp")?
        .format("%Y-%m-%d")
        .to_string())
}

pub fn list(conn: &mut SqliteConnection, section: Section) -> Result<Vec<Record>> {
    Ok(match section {
        Section::Users => users::table.order(users::username.asc()).load::<models::User>(conn)?
            .into_iter().map(|u| row(u.id, format!("{} [{}] {}", u.display_name(), u.role,
                if u.disabled { "DISABLED" } else { "active" }), json!({
                    "username": u.username, "role": u.role, "email": u.email.unwrap_or_default(),
                    "display_name": u.display_name.unwrap_or_default(),
                    "password": "", "disabled": u.disabled, "creatine_reminder": u.creatine_reminder != 0,
                }))).collect(),
        Section::Pages => {
            let mut rows = Vec::new();
            for p in pages::list(conn, true)? {
                rows.push(Record { id: p.id, summary: format!("{} [{}]", p.slug, state(p.hidden)),
                    values: json!({"slug": p.slug, "editor": "", "hidden": p.hidden, "content": p.content}),
                    revision: Some(pages::latest_revision(conn, p.id)?) });
            }
            rows
        },
        Section::Appointments => calendar_appointments::table
            .order((calendar_appointments::date.desc(), calendar_appointments::id.desc()))
            .load::<models::CalendarAppointment>(conn)?.into_iter().map(|a|
                row(a.id, format!("{} {} (person {})", a.date, a.title, a.person_id), json!({
                    "person_id": a.person_id, "title": a.title, "date": a.date,
                    "start_time": a.start_time.unwrap_or_default(), "end_time": a.end_time.unwrap_or_default(),
                }))).collect(),
        Section::People => calendar_persons::table
            .order((calendar_persons::display_order.asc(), calendar_persons::id.asc()))
            .load::<models::CalendarPerson>(conn)?.into_iter().map(|p|
                row(p.id, format!("{} (order {})", p.name, p.display_order),
                    json!({"name": p.name, "display_order": p.display_order}))).collect(),
        Section::AllowedIps => calendar_allowed_ips::table.order(calendar_allowed_ips::ip_address.asc())
            .load::<models::CalendarAllowedIp>(conn)?.into_iter().map(|ip|
                row(ip.id, ip.ip_address.clone(), json!({"ip_address": ip.ip_address}))).collect(),
        Section::Timers => {
            let data = fitness_timers::table.order(fitness_timers::id.desc())
                .select((fitness_timers::id, fitness_timers::owner_id, fitness_timers::title,
                    fitness_timers::description, fitness_timers::workout))
                .load::<(i32, i32, String, String, String)>(conn)?;
            let mut rows = Vec::new();
            for (id, owner, title, description, workout) in data {
                rows.push(row(id, format!("{title} (owner {owner})"), json!({
                    "owner_id": owner, "title": title, "description": description,
                    "workout": serde_json::from_str::<Value>(&workout)?,
                })));
            }
            rows
        },
        Section::Albums => comic_albums::table.order(comic_albums::id.desc())
            .select((comic_albums::id, comic_albums::owner_id, comic_albums::title,
                comic_albums::subtext, comic_albums::hidden))
            .load::<(i32, i32, String, String, bool)>(conn)?.into_iter().map(|(id, owner, title, subtext, hidden)|
                row(id, format!("{title} [{}] (owner {owner})", state(hidden)),
                    json!({"owner_id": owner, "title": title, "subtext": subtext, "hidden": hidden}))).collect(),
        Section::Strips => comic_strips::table.order((comic_strips::album_id.asc(), comic_strips::display_order.asc(), comic_strips::id.asc()))
            .select((comic_strips::id, comic_strips::album_id, comic_strips::title,
                comic_strips::subtext, comic_strips::hidden, comic_strips::display_order))
            .load::<(i32, i32, String, String, bool, i32)>(conn)?.into_iter().map(|(id, album, title, subtext, hidden, order)|
                row(id, format!("{title} [{}] (album {album}, order {order})", state(hidden)),
                    json!({"album_id": album, "title": title, "subtext": subtext, "hidden": hidden, "display_order": order}))).collect(),
        Section::Packages => {
            let mut rows = Vec::new();
            for p in packages::table.order(packages::ordered_date.desc()).load::<models::Package>(conn)? {
                rows.push(row(p.id, format!("{} (user {}, {})", p.name, p.user_id,
                    if p.received_date.is_some() { "received" } else { "pending" }), json!({
                        "user_id": p.user_id, "name": p.name, "tracking_id": p.tracking_id.unwrap_or_default(),
                        "ordered_date": date_string(p.ordered_date)?,
                        "received_date": p.received_date.map(date_string).transpose()?.unwrap_or_default(),
                    })));
            }
            rows
        },
        Section::Creatine => creatine_intakes::table.order(creatine_intakes::date.desc())
            .load::<models::CreatineIntake>(conn)?.into_iter().map(|c|
                row(c.id, format!("{}: {}g (user {})", c.date, c.amount_grams, c.user_id),
                    json!({"user_id": c.user_id, "date": c.date, "amount_grams": c.amount_grams}))).collect(),
    })
}

pub fn defaults(section: Section, attribution: &str) -> Value {
    let today = Utc::now().format("%Y-%m-%d").to_string();
    match section {
        Section::Users => {
            json!({"username":"","display_name":"","role":"member","email":"","password":"","disabled":false,"creatine_reminder":false})
        }
        Section::Pages => {
            json!({"slug":"","editor":attribution,"hidden":true,"content":"# New page\n"})
        }
        Section::Appointments => {
            json!({"person_id":1,"title":"","date":today,"start_time":"","end_time":""})
        }
        Section::People => json!({"name":"","display_order":0}),
        Section::AllowedIps => json!({"ip_address":""}),
        Section::Timers => json!({"owner_id":1,"title":"","description":"","workout":{
            "repeats":1,"blocks":[{"repeats":1,"steps":[{"label":"Work","seconds":30,"repeats":1},{"label":"Rest","seconds":null,"repeats":1}]}]
        }}),
        Section::Albums => json!({"owner_id":1,"title":"","subtext":"","hidden":true}),
        Section::Strips => {
            json!({"album_id":1,"title":"","subtext":"","hidden":true,"display_order":0})
        }
        Section::Packages => {
            json!({"user_id":1,"name":"","tracking_id":"","ordered_date":today,"received_date":""})
        }
        Section::Creatine => json!({"user_id":1,"date":today,"amount_grams":5.0}),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UserInput {
    username: String,
    #[serde(default)]
    display_name: String,
    role: String,
    email: String,
    password: String,
    disabled: bool,
    creatine_reminder: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageInput {
    slug: String,
    editor: String,
    hidden: bool,
    content: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AppointmentInput {
    person_id: i32,
    title: String,
    date: String,
    start_time: String,
    end_time: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PersonInput {
    name: String,
    display_order: i32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IpInput {
    ip_address: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TimerInput {
    owner_id: i32,
    title: String,
    description: String,
    workout: Workout,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AlbumInput {
    owner_id: i32,
    title: String,
    subtext: String,
    hidden: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StripInput {
    album_id: i32,
    title: String,
    subtext: String,
    hidden: bool,
    display_order: i32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PackageInput {
    user_id: i32,
    name: String,
    tracking_id: String,
    ordered_date: String,
    received_date: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreatineInput {
    user_id: i32,
    date: String,
    amount_grams: f64,
}

fn text(value: &str, name: &str, max: usize) -> Result<()> {
    if value.trim().is_empty() || value.chars().count() > max || value.contains('\0') {
        return Err(format!("{name} must contain 1-{max} characters and no NUL characters").into());
    }
    Ok(())
}

fn date(value: &str) -> Result<NaiveDate> {
    let parsed = NaiveDate::parse_from_str(value, "%Y-%m-%d")?;
    if parsed.format("%Y-%m-%d").to_string() != value {
        return Err("Use date format YYYY-MM-DD".into());
    }
    Ok(parsed)
}

fn time(value: &str) -> Result<Option<String>> {
    if value.is_empty() {
        return Ok(None);
    }
    let parsed = NaiveTime::parse_from_str(value, "%H:%M")?;
    if parsed.format("%H:%M").to_string() != value {
        return Err("Use time format HH:MM".into());
    }
    Ok(Some(value.into()))
}

fn user_exists(conn: &mut SqliteConnection, id: i32) -> Result<()> {
    users::table.find(id).select(users::id).first::<i32>(conn)?;
    Ok(())
}

fn protect_admin(conn: &mut SqliteConnection, id: i32, role: &str, disabled: bool) -> Result<()> {
    let old = users::table.find(id).first::<models::User>(conn)?;
    if !old.disabled && old.role == "admin" && (disabled || role != "admin") {
        let count = users::table
            .filter(users::disabled.eq(false))
            .filter(users::role.eq("admin"))
            .count()
            .get_result::<i64>(conn)?;
        if count <= 1 {
            return Err("Cannot disable or demote the last active admin".into());
        }
    }
    Ok(())
}

fn check_original(conn: &mut SqliteConnection, section: Section, original: &Record) -> Result<()> {
    let current = list(conn, section)?
        .into_iter()
        .find(|r| r.id == original.id)
        .ok_or("This record was deleted; refresh the list")?;
    if current.values != original.values || current.revision != original.revision {
        return Err("Record changed since it was opened; cancel and reload before saving".into());
    }
    Ok(())
}

pub fn save(
    conn: &mut SqliteConnection,
    section: Section,
    original: Option<&Record>,
    values: Value,
) -> Result<()> {
    save_authorized(conn, section, original, values, |_| Ok(()))
}

pub(crate) fn save_authorized(
    conn: &mut SqliteConnection,
    section: Section,
    original: Option<&Record>,
    values: Value,
    authorize: impl FnOnce(&mut SqliteConnection) -> Result<()>,
) -> Result<()> {
    conn.immediate_transaction::<_, pages::Error, _>(|conn| {
        authorize(conn)?;
        if let Some(record) = original { check_original(conn, section, record)?; }
        let id = original.map(|r| r.id);
        let now = Utc::now().timestamp();
        match section {
            Section::Users => {
                let p: UserInput = serde_json::from_value(values)?;
                text(&p.username, "Username", 100)?;
                let display_name = models::normalize_display_name(Some(&p.display_name))?;
                if !matches!(p.role.as_str(), "admin" | "member" | "user") {
                    return Err("Role must be admin or member (legacy user is also accepted)".into());
                }
                if !p.email.is_empty() { p.email.parse::<lettre::Address>()?; }
                if id.is_none() && p.password.is_empty() { return Err("A password is required for new users".into()); }
                if !p.password.is_empty() && p.password.len() > 72 { return Err("Bcrypt passwords must be at most 72 bytes".into()); }
                let email = (!p.email.is_empty()).then_some(p.email.as_str());
                let password = if p.password.is_empty() { None } else { Some(bcrypt::hash(&p.password, bcrypt::DEFAULT_COST)?) };
                if let Some(id) = id {
                    protect_admin(conn, id, &p.role, p.disabled)?;
                    let old = users::table.find(id).first::<models::User>(conn)?;
                    let revoke = old.disabled != p.disabled || old.role != p.role || password.is_some();
                    diesel::update(users::table.find(id)).set((
                        users::username.eq(p.username.trim()), users::role.eq(&p.role), users::email.eq(email),
                        users::display_name.eq(display_name),
                        users::disabled.eq(p.disabled), users::creatine_reminder.eq(i32::from(p.creatine_reminder)),
                        users::session_version.eq(old.session_version + i64::from(revoke)),
                    )).execute(conn)?;
                    if let Some(password) = password {
                        diesel::update(users::table.find(id)).set(users::password_hash.eq(password)).execute(conn)?;
                    }
                } else {
                    diesel::insert_into(users::table).values((
                        users::username.eq(p.username.trim()), users::role.eq(&p.role), users::email.eq(email),
                        users::display_name.eq(display_name),
                        users::password_hash.eq(password.ok_or("A password is required")?), users::created_at.eq(now),
                        users::disabled.eq(p.disabled), users::creatine_reminder.eq(i32::from(p.creatine_reminder)),
                    )).execute(conn)?;
                }
            },
            Section::Pages => {
                let p: PageInput = serde_json::from_value(values)?;
                text(&p.editor, "Editor attribution", 100)?;
                if let Some(id) = id {
                    let old = original.ok_or("Missing page snapshot")?;
                    if p.slug != old.values["slug"] { return Err("Existing page slugs cannot be renamed here".into()); }
                    pages::edit(conn, id, old.revision.ok_or("Missing page revision")?, &p.content, p.hidden, &p.editor)?;
                } else {
                    pages::create(conn, &p.slug, &p.content, &p.editor, p.hidden)?;
                }
            },
            Section::Appointments => {
                let p: AppointmentInput = serde_json::from_value(values)?;
                text(&p.title, "Title", 200)?;
                date(&p.date)?;
                calendar_persons::table.find(p.person_id).select(calendar_persons::id).first::<i32>(conn)?;
                let start = time(&p.start_time)?;
                let end = time(&p.end_time)?;
                if end.is_some() && start.is_none() { return Err("An end time requires a start time".into()); }
                if start.as_ref().zip(end.as_ref()).is_some_and(|(s, e)| e <= s) {
                    return Err("End time must be after start time on the same day".into());
                }
                let changes = (calendar_appointments::person_id.eq(p.person_id),
                    calendar_appointments::title.eq(p.title.trim()), calendar_appointments::date.eq(&p.date),
                    calendar_appointments::start_time.eq(start), calendar_appointments::end_time.eq(end));
                if let Some(id) = id {
                    diesel::update(calendar_appointments::table.find(id)).set(changes).execute(conn)?;
                } else {
                    diesel::insert_into(calendar_appointments::table)
                        .values((changes, calendar_appointments::created_at.eq(now))).execute(conn)?;
                }
            },
            Section::People => {
                let p: PersonInput = serde_json::from_value(values)?;
                text(&p.name, "Name", 100)?;
                if p.display_order < 0 { return Err("Display order must be 0 or greater".into()); }
                let changes = (calendar_persons::name.eq(p.name.trim()), calendar_persons::display_order.eq(p.display_order));
                if let Some(id) = id { diesel::update(calendar_persons::table.find(id)).set(changes).execute(conn)?; }
                else { diesel::insert_into(calendar_persons::table).values(changes).execute(conn)?; }
            },
            Section::AllowedIps => {
                let p: IpInput = serde_json::from_value(values)?;
                let ip: std::net::IpAddr = p.ip_address.trim().parse()?;
                let canonical = ip.to_string();
                let change = calendar_allowed_ips::ip_address.eq(canonical);
                if let Some(id) = id { diesel::update(calendar_allowed_ips::table.find(id)).set(change).execute(conn)?; }
                else { diesel::insert_into(calendar_allowed_ips::table)
                    .values((change, calendar_allowed_ips::created_at.eq(now))).execute(conn)?; }
            },
            Section::Timers => {
                let p: TimerInput = serde_json::from_value(values)?;
                user_exists(conn, p.owner_id)?;
                text(&p.title, "Title", 100)?;
                if p.description.chars().count() > 2000 { return Err("Description must be at most 2,000 characters".into()); }
                p.workout.validate()?;
                let changes = (fitness_timers::owner_id.eq(p.owner_id), fitness_timers::title.eq(p.title.trim()),
                    fitness_timers::description.eq(&p.description), fitness_timers::workout.eq(serde_json::to_string(&p.workout)?),
                    fitness_timers::updated_at.eq(now));
                if let Some(id) = id { diesel::update(fitness_timers::table.find(id)).set(changes).execute(conn)?; }
                else { diesel::insert_into(fitness_timers::table).values((changes, fitness_timers::created_at.eq(now))).execute(conn)?; }
            },
            Section::Albums => {
                let p: AlbumInput = serde_json::from_value(values)?;
                user_exists(conn, p.owner_id)?;
                text(&p.title, "Title", 200)?;
                let changes = (comic_albums::owner_id.eq(p.owner_id), comic_albums::title.eq(p.title.trim()),
                    comic_albums::subtext.eq(&p.subtext), comic_albums::hidden.eq(p.hidden), comic_albums::updated_at.eq(now));
                if let Some(id) = id { diesel::update(comic_albums::table.find(id)).set(changes).execute(conn)?; }
                else { diesel::insert_into(comic_albums::table).values((changes, comic_albums::created_at.eq(now))).execute(conn)?; }
            },
            Section::Strips => {
                let id = id.ok_or("Upload new strip images through the website; manage edits existing strip metadata only")?;
                let p: StripInput = serde_json::from_value(values)?;
                let old_album = comic_strips::table.find(id).select(comic_strips::album_id).first::<i32>(conn)?;
                if p.album_id != old_album { return Err("Moving strips between albums would break image storage; album ID cannot change".into()); }
                text(&p.title, "Title", 200)?;
                if p.display_order < 0 { return Err("Display order must be 0 or greater".into()); }
                diesel::update(comic_strips::table.find(id)).set((
                    comic_strips::title.eq(p.title.trim()), comic_strips::subtext.eq(&p.subtext),
                    comic_strips::hidden.eq(p.hidden), comic_strips::display_order.eq(p.display_order),
                    comic_strips::updated_at.eq(now),
                )).execute(conn)?;
            },
            Section::Packages => {
                let p: PackageInput = serde_json::from_value(values)?;
                user_exists(conn, p.user_id)?;
                text(&p.name, "Package name", 200)?;
                let ordered = date(&p.ordered_date)?.and_hms_opt(12, 0, 0).ok_or("Invalid order date")?.and_utc().timestamp();
                let received = if p.received_date.is_empty() { None } else {
                    Some(date(&p.received_date)?.and_hms_opt(12, 0, 0).ok_or("Invalid receipt date")?.and_utc().timestamp())
                };
                if received.is_some_and(|r| r < ordered) { return Err("Received date cannot precede ordered date".into()); }
                let tracking = (!p.tracking_id.is_empty()).then_some(&p.tracking_id);
                if let Some(id) = id {
                    let old = packages::table.find(id).first::<models::Package>(conn)?;
                    let ordered = if date_string(old.ordered_date)? == p.ordered_date { old.ordered_date } else { ordered };
                    let received = if old.received_date.map(date_string).transpose()?.unwrap_or_default() == p.received_date { old.received_date } else { received };
                    diesel::update(packages::table.find(id)).set((
                        packages::user_id.eq(p.user_id), packages::name.eq(p.name.trim()), packages::tracking_id.eq(tracking),
                        packages::ordered_date.eq(ordered), packages::received_date.eq(received),
                    )).execute(conn)?;
                } else {
                    diesel::insert_into(packages::table).values((
                        packages::user_id.eq(p.user_id), packages::name.eq(p.name.trim()), packages::tracking_id.eq(tracking),
                        packages::ordered_date.eq(ordered), packages::received_date.eq(received),
                    )).execute(conn)?;
                }
            },
            Section::Creatine => {
                let p: CreatineInput = serde_json::from_value(values)?;
                user_exists(conn, p.user_id)?;
                date(&p.date)?;
                if !p.amount_grams.is_finite() || p.amount_grams <= 0.0 { return Err("Amount must be a finite number greater than zero".into()); }
                let changes = (creatine_intakes::user_id.eq(p.user_id), creatine_intakes::date.eq(&p.date),
                    creatine_intakes::amount_grams.eq(p.amount_grams), creatine_intakes::recorded_at.eq(now));
                if let Some(id) = id { diesel::update(creatine_intakes::table.find(id)).set(changes).execute(conn)?; }
                else { diesel::insert_into(creatine_intakes::table).values(changes).execute(conn)?; }
            },
        }
        Ok(())
    })
}

pub fn remove(
    conn: &mut SqliteConnection,
    section: Section,
    original: &Record,
    attribution: &str,
) -> Result<()> {
    remove_authorized(conn, section, original, attribution, |_| Ok(()))
}

pub(crate) fn remove_authorized(
    conn: &mut SqliteConnection,
    section: Section,
    original: &Record,
    attribution: &str,
    authorize: impl FnOnce(&mut SqliteConnection) -> Result<()>,
) -> Result<()> {
    if section == Section::Users {
        let mut values = original.values.clone();
        values["disabled"] = json!(true);
        return save_authorized(conn, section, Some(original), values, authorize);
    }
    conn.immediate_transaction::<_, pages::Error, _>(|conn| {
        authorize(conn)?;
        check_original(conn, section, original)?;
        let id = original.id;
        match section {
            Section::Users => return Err("Users must be disabled through the account save operation".into()),
            Section::Pages => {
                pages::edit(conn, id, original.revision.ok_or("Missing revision")?,
                    original.values["content"].as_str().ok_or("Missing content")?, true, attribution)?;
            },
            Section::Albums => { diesel::update(comic_albums::table.find(id))
                .set((comic_albums::hidden.eq(true), comic_albums::updated_at.eq(Utc::now().timestamp()))).execute(conn)?; },
            Section::Strips => { diesel::update(comic_strips::table.find(id))
                .set((comic_strips::hidden.eq(true), comic_strips::updated_at.eq(Utc::now().timestamp()))).execute(conn)?; },
            Section::Appointments => { diesel::delete(calendar_appointments::table.find(id)).execute(conn)?; },
            Section::People => {
                if calendar_appointments::table.filter(calendar_appointments::person_id.eq(id)).count().get_result::<i64>(conn)? > 0 {
                    return Err("Cannot delete a person with appointments; reassign or delete those appointments first".into());
                }
                diesel::delete(calendar_persons::table.find(id)).execute(conn)?;
            },
            Section::AllowedIps => { diesel::delete(calendar_allowed_ips::table.find(id)).execute(conn)?; },
            Section::Timers => { diesel::delete(fitness_timers::table.find(id)).execute(conn)?; },
            Section::Packages => { diesel::delete(packages::table.find(id)).execute(conn)?; },
            Section::Creatine => { diesel::delete(creatine_intakes::table.find(id)).execute(conn)?; },
        }
        Ok(())
    })
}

pub fn details(conn: &mut SqliteConnection, section: Section, record: &Record) -> Result<String> {
    if section == Section::Pages {
        #[derive(Serialize)]
        struct History {
            page: pages::Page,
            revisions: Vec<pages::Revision>,
        }
        Ok(serde_json::to_string_pretty(&History {
            page: pages::by_id(conn, record.id)?,
            revisions: pages::history(conn, record.id)?,
        })?)
    } else {
        Ok(serde_json::to_string_pretty(&record.values)?)
    }
}
