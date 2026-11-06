use diesel::prelude::*;
use rocket::http::{CookieJar, Status};
use rocket::serde::json::Json;
use rocket::{State, get, post};
use rocket_dyn_templates::{Template, context};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::comics::{Private, Viewer};
use crate::db::DbPool;
use crate::schema::{minesweeper_games as games, minesweeper_scores as scores};

type GameError = (Status, String);
type GameResult<T> = Result<T, GameError>;

pub fn routes() -> Vec<rocket::Route> {
    rocket::routes![
        index,
        makibishi,
        start,
        browser_board,
        play,
        submit_score,
        leaderboard,
        score_page,
        native_start,
        native_play,
        native_score,
        native_board
    ]
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = games)]
struct Game {
    id: String,
    mode: String,
    rows: i32,
    board_columns: i32,
    mine_count: i32,
    mines: String,
    revealed: String,
    flags: String,
    user_id: Option<i32>,
    started_at: i64,
    created_at: i64,
    status: String,
    elapsed_ms: Option<i64>,
}

#[derive(Insertable)]
#[diesel(table_name = games)]
struct NewGame<'a> {
    id: &'a str,
    mode: &'a str,
    rows: i32,
    board_columns: i32,
    mine_count: i32,
    mines: &'a str,
    revealed: &'a str,
    flags: &'a str,
    user_id: Option<i32>,
    started_at: i64,
    created_at: i64,
    status: &'a str,
    elapsed_ms: Option<i64>,
}

#[derive(Queryable)]
struct StoredScore {
    id: i32,
    game_id: String,
    mode: String,
    player_name: String,
    user_id: Option<i32>,
    elapsed_ms: i64,
    created_at: i64,
    winning_board: Option<String>,
}

#[derive(Serialize)]
struct Score {
    id: i32,
    game_id: String,
    mode: String,
    player_name: String,
    user_id: Option<i32>,
    elapsed_ms: i64,
    created_at: i64,
    board: Option<WinningBoard>,
}

impl Score {
    fn from_stored(score: StoredScore) -> GameResult<Self> {
        Ok(Self {
            id: score.id,
            game_id: score.game_id,
            mode: score.mode,
            player_name: score.player_name,
            user_id: score.user_id,
            elapsed_ms: score.elapsed_ms,
            created_at: score.created_at,
            board: score
                .winning_board
                .map(|json| serde_json::from_str(&json).map_err(internal))
                .transpose()?,
        })
    }
}

#[derive(Deserialize, Serialize)]
struct WinningBoard {
    rows: usize,
    columns: usize,
    cells: Vec<Cell>,
}

#[derive(Deserialize, Serialize)]
struct Cell {
    revealed: bool,
    flagged: bool,
    mine: bool,
    count: u8,
}

#[derive(Serialize)]
struct GameView {
    id: String,
    mode: String,
    rows: usize,
    columns: usize,
    mine_count: usize,
    cells: Vec<Cell>,
    status: String,
    elapsed_ms: Option<i64>,
    clock_started: bool,
}

#[derive(Deserialize)]
struct StartRequest {
    mode: String,
    rows: Option<usize>,
    columns: Option<usize>,
    mine_count: Option<usize>,
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    require_signed_in: bool,
}

#[derive(Deserialize)]
struct MoveRequest {
    action: String,
    row: usize,
    column: usize,
    #[serde(default)]
    csrf: String,
}

#[derive(Deserialize)]
struct ScoreRequest {
    player_name: Option<String>,
    #[serde(default)]
    csrf: String,
}

#[derive(Serialize)]
struct Leaderboards {
    easy: Vec<Score>,
    intermediate: Vec<Score>,
    advanced: Vec<Score>,
}

fn internal(error: impl std::fmt::Display) -> GameError {
    eprintln!("Minesweeper operation failed: {error}");
    (
        Status::InternalServerError,
        "The game could not be saved or loaded. Please try again.".into(),
    )
}

fn csrf(jar: &CookieJar<'_>, token: &str) -> GameResult<()> {
    if jar.get_private("csrf").is_some_and(|c| c.value() == token) {
        Ok(())
    } else {
        Err((
            Status::Forbidden,
            "Invalid form token. Reload the page and try again.".into(),
        ))
    }
}

fn settings(request: &StartRequest) -> GameResult<(&str, usize, usize, usize)> {
    let (mode, rows, columns, mines) = match request.mode.as_str() {
        "easy" => ("easy", 9, 9, 10),
        "intermediate" => ("intermediate", 16, 16, 40),
        "advanced" => ("advanced", 16, 30, 99),
        "custom" => (
            "custom",
            request.rows.unwrap_or_default(),
            request.columns.unwrap_or_default(),
            request.mine_count.unwrap_or_default(),
        ),
        _ => {
            return Err((Status::UnprocessableEntity, "Unknown game mode.".into()));
        }
    };
    if !(5..=24).contains(&rows)
        || !(5..=30).contains(&columns)
        || mines == 0
        || mines >= rows * columns
        || rows * columns > 720
    {
        return Err((
            Status::UnprocessableEntity,
            "Custom boards must be 5–24 rows, 5–30 columns, and have fewer mines than cells."
                .into(),
        ));
    }
    Ok((mode, rows, columns, mines))
}

fn shuffled_mines(id: &Uuid, cells: usize, mine_count: usize) -> Vec<bool> {
    let mut seed = id
        .as_bytes()
        .iter()
        .fold(0x9e3779b97f4a7c15_u64, |state, byte| {
            state.rotate_left(7) ^ u64::from(*byte)
        });
    let mut positions: Vec<usize> = (0..cells).collect();
    for index in 0..mine_count {
        seed ^= seed >> 12;
        seed ^= seed << 25;
        seed ^= seed >> 27;
        let next = seed.wrapping_mul(0x2545f4914f6cdd1d);
        let chosen = index + (next as usize % (cells - index));
        positions.swap(index, chosen);
    }
    let mut mines = vec![false; cells];
    for index in positions.into_iter().take(mine_count) {
        mines[index] = true;
    }
    mines
}

fn deserialize_state(game: &Game) -> GameResult<(Vec<bool>, Vec<bool>, Vec<bool>)> {
    let mines: Vec<bool> = serde_json::from_str(&game.mines).map_err(internal)?;
    let revealed: Vec<bool> = serde_json::from_str(&game.revealed).map_err(internal)?;
    let flags: Vec<bool> = serde_json::from_str(&game.flags).map_err(internal)?;
    let cells = (game.rows * game.board_columns) as usize;
    if mines.len() != cells || revealed.len() != cells || flags.len() != cells {
        return Err(internal(
            "Stored Minesweeper board has inconsistent dimensions",
        ));
    }
    Ok((mines, revealed, flags))
}

fn adjacent(row: usize, column: usize, rows: usize, columns: usize) -> Vec<usize> {
    let mut result = Vec::with_capacity(8);
    for r in row.saturating_sub(1)..=(row + 1).min(rows - 1) {
        for c in column.saturating_sub(1)..=(column + 1).min(columns - 1) {
            if r != row || c != column {
                result.push(r * columns + c);
            }
        }
    }
    result
}

fn adjacent_mines(index: usize, mines: &[bool], rows: usize, columns: usize) -> u8 {
    adjacent(index / columns, index % columns, rows, columns)
        .into_iter()
        .filter(|&neighbor| mines[neighbor])
        .count() as u8
}

fn reveal_area(
    index: usize,
    mines: &[bool],
    revealed: &mut [bool],
    flags: &[bool],
    rows: usize,
    columns: usize,
) -> bool {
    if flags[index] || revealed[index] {
        return false;
    }
    if mines[index] {
        return true;
    }
    let mut pending = vec![index];
    while let Some(cell) = pending.pop() {
        if revealed[cell] || flags[cell] || mines[cell] {
            continue;
        }
        revealed[cell] = true;
        if adjacent_mines(cell, mines, rows, columns) == 0 {
            pending.extend(adjacent(cell / columns, cell % columns, rows, columns));
        }
    }
    false
}

fn game_view(game: &Game) -> GameResult<GameView> {
    let (mines, revealed, flags) = deserialize_state(game)?;
    let rows = game.rows as usize;
    let columns = game.board_columns as usize;
    let cells = mines
        .iter()
        .enumerate()
        .map(|(index, &mine)| {
            let visible = revealed[index] || game.status == "lost";
            Cell {
                revealed: revealed[index] || game.status == "lost",
                flagged: flags[index],
                mine: visible && mine,
                count: if visible && !mine {
                    adjacent_mines(index, &mines, rows, columns)
                } else {
                    0
                },
            }
        })
        .collect();
    Ok(GameView {
        id: game.id.clone(),
        mode: game.mode.clone(),
        rows,
        columns,
        mine_count: game.mine_count as usize,
        cells,
        status: game.status.clone(),
        elapsed_ms: game.elapsed_ms,
        clock_started: game.started_at > 0,
    })
}

fn load_leaderboard(conn: &mut SqliteConnection, mode_name: &str) -> GameResult<Vec<Score>> {
    let mut scores = scores::table
        .filter(scores::mode.eq(mode_name))
        .order((scores::elapsed_ms.asc(), scores::created_at.asc()))
        .limit(10)
        .load::<StoredScore>(conn)
        .map_err(internal)?
        .into_iter()
        .map(Score::from_stored)
        .collect::<GameResult<Vec<_>>>()?;
    let names =
        crate::models::account_display_names(conn, scores.iter().filter_map(|score| score.user_id))
            .map_err(internal)?;
    for score in &mut scores {
        if let Some(name) = score.user_id.and_then(|id| names.get(&id)) {
            score.player_name.clone_from(name);
        }
    }
    Ok(scores)
}

#[get("/games/minesweeper/scores/<id>")]
fn score_page(
    id: i32,
    pool: &State<DbPool>,
    navigation: crate::PageNavigation,
) -> GameResult<Template> {
    let mut conn = pool.get().map_err(internal)?;
    let stored = scores::table
        .find(id)
        .first::<StoredScore>(&mut conn)
        .optional()
        .map_err(internal)?
        .ok_or((Status::NotFound, "Score not found.".into()))?;
    let score = Score::from_stored(stored)?;
    Ok(Template::render(
        "games/minesweeper_score",
        context! { pages: navigation.0, score: score },
    ))
}

#[get("/games/minesweeper")]
fn index(
    pool: &State<DbPool>,
    viewer: Viewer,
    navigation: crate::PageNavigation,
    jar: &CookieJar<'_>,
) -> GameResult<Private<Template>> {
    let mut conn = pool.get().map_err(internal)?;
    let easy_scores = load_leaderboard(&mut conn, "easy")?;
    let intermediate_scores = load_leaderboard(&mut conn, "intermediate")?;
    let advanced_scores = load_leaderboard(&mut conn, "advanced")?;
    Ok(Private(Template::render(
        "games/minesweeper",
        context! {
            pages: navigation.0,
            csrf: crate::ensure_csrf(jar),
            signed_in: viewer.identity().is_some(),
            username: viewer.display_name().unwrap_or(""),
            easy_scores: easy_scores,
            intermediate_scores: intermediate_scores,
            advanced_scores: advanced_scores,
        },
    )))
}

#[post("/games/minesweeper/api/games", format = "json", data = "<request>")]
fn start(
    pool: &State<DbPool>,
    jar: &CookieJar<'_>,
    viewer: Viewer,
    request: Json<StartRequest>,
) -> GameResult<Private<Json<GameView>>> {
    csrf(jar, &request.csrf)?;
    let user_id = viewer.identity().map(|(id, _)| id);
    if request.require_signed_in && user_id.is_none() {
        return Err((
            Status::Unauthorized,
            "Your website session expired. Log in again before starting a ranked game.".into(),
        ));
    }
    start_game(pool, &request, user_id)
}

#[get("/games/makibishi")]
fn makibishi(
    viewer: Viewer,
    navigation: crate::PageNavigation,
    jar: &CookieJar<'_>,
) -> Private<Template> {
    Private(Template::render(
        "games/makibishi",
        context! {
            pages: navigation.0,
            csrf: crate::ensure_csrf(jar),
            signed_in: viewer.identity().is_some(),
            username: viewer.display_name().unwrap_or(""),
        },
    ))
}

#[get("/games/minesweeper/api/games/<id>")]
fn browser_board(
    id: String,
    pool: &State<DbPool>,
    viewer: Viewer,
) -> GameResult<Private<Json<GameView>>> {
    owned_board(&id, pool, viewer.identity().map(|(id, _)| id))
}

fn start_game(
    pool: &DbPool,
    request: &StartRequest,
    user_id: Option<i32>,
) -> GameResult<Private<Json<GameView>>> {
    let (mode, rows, columns, mine_count) = settings(&request)?;
    let id = Uuid::new_v4();
    let mines = shuffled_mines(&id, rows * columns, mine_count);
    let blank = vec![false; rows * columns];
    let mines_json = serde_json::to_string(&mines).map_err(internal)?;
    let blank_json = serde_json::to_string(&blank).map_err(internal)?;
    let game_id = id.as_hyphenated().to_string();
    let game = NewGame {
        id: &game_id,
        mode,
        rows: rows as i32,
        board_columns: columns as i32,
        mine_count: mine_count as i32,
        mines: &mines_json,
        revealed: &blank_json,
        flags: &blank_json,
        user_id,
        started_at: 0,
        created_at: chrono::Utc::now().timestamp_millis(),
        status: "playing",
        elapsed_ms: None,
    };
    let mut conn = pool.get().map_err(internal)?;
    diesel::delete(
        games::table.filter(
            diesel::dsl::sql::<diesel::sql_types::BigInt>(
                "COALESCE(NULLIF(started_at, 0), created_at)",
            )
            .lt(chrono::Utc::now().timestamp_millis() - 86_400_000),
        ),
    )
    .execute(&mut conn)
    .map_err(internal)?;
    diesel::insert_into(games::table)
        .values(&game)
        .execute(&mut conn)
        .map_err(internal)?;
    let game = games::table
        .find(&game_id)
        .first::<Game>(&mut conn)
        .map_err(internal)?;
    Ok(Private(Json(game_view(&game)?)))
}

#[post(
    "/games/minesweeper/api/games/<id>/move",
    format = "json",
    data = "<request>"
)]
fn play(
    id: String,
    pool: &State<DbPool>,
    jar: &CookieJar<'_>,
    viewer: Viewer,
    request: Json<MoveRequest>,
) -> GameResult<Private<Json<GameView>>> {
    csrf(jar, &request.csrf)?;
    play_game(&id, pool, &request, viewer.identity().map(|(id, _)| id))
}

fn play_game(
    id: &str,
    pool: &DbPool,
    request: &MoveRequest,
    user_id: Option<i32>,
) -> GameResult<Private<Json<GameView>>> {
    let mut conn = pool.get().map_err(internal)?;
    let mut game = games::table
        .find(id)
        .first::<Game>(&mut conn)
        .optional()
        .map_err(internal)?
        .ok_or((Status::NotFound, "Game not found or expired.".into()))?;
    if game.created_at < chrono::Utc::now().timestamp_millis() - 86_400_000 {
        return Err((Status::NotFound, "Game not found or expired.".into()));
    }
    if game.user_id != user_id {
        return Err((
            Status::Forbidden,
            "This game belongs to a different player.".into(),
        ));
    }
    if !matches!(request.action.as_str(), "flag" | "reveal" | "chord") {
        return Err((
            Status::UnprocessableEntity,
            "Unknown move. Use reveal, flag, or chord.".into(),
        ));
    }
    let rows = game.rows as usize;
    let columns = game.board_columns as usize;
    if request.row >= rows || request.column >= columns {
        return Err((
            Status::UnprocessableEntity,
            "Cell is outside the board.".into(),
        ));
    }
    if game.status != "playing" {
        return Ok(Private(Json(game_view(&game)?)));
    }
    let original_started_at = game.started_at;
    if game.started_at == 0 {
        game.started_at = chrono::Utc::now().timestamp_millis();
    }
    let index = request.row * columns + request.column;
    let (mut mines, mut revealed, mut flags) = deserialize_state(&game)?;
    match request.action.as_str() {
        "flag" => {
            if !revealed[index] {
                flags[index] = !flags[index];
            }
        }
        "reveal" => {
            if !revealed.iter().any(|&cell| cell) && mines[index] {
                let safe = mines.iter().position(|&mine| !mine).unwrap();
                mines.swap(index, safe);
            }
            if reveal_area(index, &mines, &mut revealed, &flags, rows, columns) {
                game.status = "lost".into();
                revealed.fill(true);
            }
        }
        "chord" => {
            if revealed[index]
                && adjacent(request.row, request.column, rows, columns)
                    .into_iter()
                    .filter(|&cell| flags[cell])
                    .count()
                    == usize::from(adjacent_mines(index, &mines, rows, columns))
            {
                for neighbor in adjacent(request.row, request.column, rows, columns) {
                    if reveal_area(neighbor, &mines, &mut revealed, &flags, rows, columns) {
                        game.status = "lost".into();
                        revealed.fill(true);
                        break;
                    }
                }
            }
        }
        _ => unreachable!("move action was validated"),
    }
    if game.status == "playing"
        && revealed.iter().filter(|&&cell| cell).count()
            == (rows * columns - game.mine_count as usize)
    {
        game.status = "won".into();
    }
    let elapsed = if game.status == "won" {
        Some((chrono::Utc::now().timestamp_millis() - game.started_at).max(0))
    } else {
        None
    };
    let original_mines = game.mines.clone();
    let original_revealed = game.revealed.clone();
    let original_flags = game.flags.clone();
    game.mines = serde_json::to_string(&mines).map_err(internal)?;
    game.revealed = serde_json::to_string(&revealed).map_err(internal)?;
    game.flags = serde_json::to_string(&flags).map_err(internal)?;
    game.elapsed_ms = elapsed;
    let changed = diesel::update(
        games::table
            .filter(games::id.eq(id))
            .filter(games::status.eq("playing"))
            .filter(games::started_at.eq(original_started_at))
            .filter(games::mines.eq(original_mines))
            .filter(games::revealed.eq(original_revealed))
            .filter(games::flags.eq(original_flags)),
    )
    .set((
        games::mines.eq(&game.mines),
        games::revealed.eq(&game.revealed),
        games::flags.eq(&game.flags),
        games::started_at.eq(game.started_at),
        games::status.eq(&game.status),
        games::elapsed_ms.eq(elapsed),
    ))
    .execute(&mut conn)
    .map_err(internal)?;
    if changed == 0 {
        return Err((
            Status::Conflict,
            "The board changed before this move was saved. Please try again.".into(),
        ));
    }
    Ok(Private(Json(game_view(&game)?)))
}

#[post(
    "/games/minesweeper/api/games/<id>/score",
    format = "json",
    data = "<request>"
)]
fn submit_score(
    id: String,
    pool: &State<DbPool>,
    jar: &CookieJar<'_>,
    viewer: Viewer,
    request: Json<ScoreRequest>,
) -> GameResult<Private<Json<Score>>> {
    csrf(jar, &request.csrf)?;
    save_score(
        &id,
        pool,
        &request,
        viewer.identity().map(|(id, _)| id),
        viewer.display_name(),
    )
}

fn save_score(
    id: &str,
    pool: &DbPool,
    request: &ScoreRequest,
    identity: Option<i32>,
    display_name: Option<&str>,
) -> GameResult<Private<Json<Score>>> {
    let mut conn = pool.get().map_err(internal)?;
    conn.immediate_transaction::<_, ScoreTransactionError, _>(|conn| {
        save_score_in_transaction(conn, id, request, identity, display_name)
            .map_err(ScoreTransactionError)
    })
    .map_err(|error| error.0)
}

struct ScoreTransactionError(GameError);

impl From<diesel::result::Error> for ScoreTransactionError {
    fn from(error: diesel::result::Error) -> Self {
        Self(internal(error))
    }
}

fn save_score_in_transaction(
    conn: &mut SqliteConnection,
    id: &str,
    request: &ScoreRequest,
    identity: Option<i32>,
    display_name: Option<&str>,
) -> GameResult<Private<Json<Score>>> {
    if let Some(existing) = scores::table
        .filter(scores::game_id.eq(id))
        .first::<StoredScore>(conn)
        .optional()
        .map_err(internal)?
    {
        if existing.user_id != identity {
            return Err((
                Status::Forbidden,
                "This score belongs to a different player.".into(),
            ));
        }
        return Ok(Private(Json(Score::from_stored(existing)?)));
    }
    let game = games::table
        .find(id)
        .first::<Game>(conn)
        .optional()
        .map_err(internal)?
        .ok_or((Status::NotFound, "Game not found or expired.".into()))?;
    if game.status != "won" || !matches!(game.mode.as_str(), "easy" | "intermediate" | "advanced") {
        return Err((
            Status::UnprocessableEntity,
            "Only completed fixed-mode games can be added to a leaderboard.".into(),
        ));
    }
    if game.user_id != identity {
        return Err((
            Status::Forbidden,
            "Scores must be submitted by the same player who started the game.".into(),
        ));
    }
    let player_name = if let Some(username) = display_name {
        username.to_owned()
    } else {
        let name = request.player_name.as_deref().unwrap_or("").trim();
        if name.is_empty() || name.chars().count() > 24 || name.chars().any(char::is_control) {
            return Err((
                Status::UnprocessableEntity,
                "Enter a name of 1–24 characters without control characters.".into(),
            ));
        }
        name.to_owned()
    };
    let (mines, revealed, flags) = deserialize_state(&game)?;
    let rows = game.rows as usize;
    let columns = game.board_columns as usize;
    let board = WinningBoard {
        rows,
        columns,
        cells: mines
            .iter()
            .enumerate()
            .map(|(index, &mine)| Cell {
                mine,
                revealed: revealed[index],
                flagged: flags[index],
                count: if mine {
                    0
                } else {
                    adjacent_mines(index, &mines, rows, columns)
                },
            })
            .collect(),
    };
    let board_json = serde_json::to_string(&board).map_err(internal)?;
    let new_score = (
        scores::game_id.eq(&game.id),
        scores::mode.eq(&game.mode),
        scores::player_name.eq(player_name),
        scores::user_id.eq(identity),
        scores::elapsed_ms.eq(game.elapsed_ms.unwrap_or_default()),
        scores::created_at.eq(chrono::Utc::now().timestamp_millis()),
        scores::winning_board.eq(board_json),
    );
    diesel::insert_into(scores::table)
        .values(new_score)
        .execute(conn)
        .map_err(internal)?;
    let score = scores::table
        .filter(scores::game_id.eq(&game.id))
        .first::<StoredScore>(conn)
        .map_err(internal)?;
    Ok(Private(Json(Score::from_stored(score)?)))
}

#[get("/games/minesweeper/api/scores")]
fn leaderboard(pool: &State<DbPool>) -> GameResult<Private<Json<Leaderboards>>> {
    let mut conn = pool.get().map_err(internal)?;
    Ok(Private(Json(Leaderboards {
        easy: load_leaderboard(&mut conn, "easy")?,
        intermediate: load_leaderboard(&mut conn, "intermediate")?,
        advanced: load_leaderboard(&mut conn, "advanced")?,
    })))
}

#[post("/api/player/minesweeper/games", format = "json", data = "<request>")]
fn native_start(
    pool: &State<DbPool>,
    player: wasd5::player_auth::Player,
    request: Json<StartRequest>,
) -> GameResult<Private<Json<GameView>>> {
    native_clock(start_game(pool, &request, Some(player.user.id)), pool)
}

#[post(
    "/api/player/minesweeper/games/<id>/move",
    format = "json",
    data = "<request>"
)]
fn native_play(
    id: String,
    pool: &State<DbPool>,
    player: wasd5::player_auth::Player,
    request: Json<MoveRequest>,
) -> GameResult<Private<Json<GameView>>> {
    native_clock(play_game(&id, pool, &request, Some(player.user.id)), pool)
}

#[post(
    "/api/player/minesweeper/games/<id>/score",
    format = "json",
    data = "<request>"
)]
fn native_score(
    id: String,
    pool: &State<DbPool>,
    player: wasd5::player_auth::Player,
    request: Json<ScoreRequest>,
) -> GameResult<Private<Json<Score>>> {
    save_score(
        &id,
        pool,
        &request,
        Some(player.user.id),
        Some(player.user.display_name()),
    )
}

#[get("/api/player/minesweeper/games/<id>")]
fn native_board(
    id: String,
    pool: &State<DbPool>,
    player: wasd5::player_auth::Player,
) -> GameResult<Private<Json<GameView>>> {
    owned_board(&id, pool, Some(player.user.id))
}

fn owned_board(
    id: &str,
    pool: &DbPool,
    user_id: Option<i32>,
) -> GameResult<Private<Json<GameView>>> {
    let mut conn = pool.get().map_err(internal)?;
    let game = games::table
        .find(id)
        .first::<Game>(&mut conn)
        .optional()
        .map_err(internal)?
        .filter(|g| g.created_at >= chrono::Utc::now().timestamp_millis() - 86_400_000)
        .ok_or((Status::NotFound, "Game not found or expired.".into()))?;
    if game.user_id != user_id {
        return Err((
            Status::Forbidden,
            "This game belongs to a different player.".into(),
        ));
    }
    native_clock(Ok(Private(Json(game_view(&game)?))), pool)
}

fn native_clock(
    result: GameResult<Private<Json<GameView>>>,
    pool: &DbPool,
) -> GameResult<Private<Json<GameView>>> {
    let mut response = result?;
    let view = &mut response.0;
    if view.clock_started && view.status == "playing" {
        let mut conn = pool.get().map_err(internal)?;
        let started_at = games::table
            .find(&view.id)
            .select(games::started_at)
            .first::<i64>(&mut conn)
            .map_err(internal)?;
        view.elapsed_ms = Some((chrono::Utc::now().timestamp_millis() - started_at).max(0));
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use rocket::http::ContentType;
    use rocket::local::blocking::Client;

    fn client() -> (tempfile::TempDir, Client, DbPool) {
        let dir = tempfile::Builder::new()
            .prefix(".minesweeper-test-")
            .tempdir_in(".")
            .unwrap();
        let pool = db::establish_pool(dir.path().join("games.db").to_str().unwrap());
        let mut conn = pool.get().unwrap();
        db::run_migrations(&mut conn).unwrap();
        let password_hash = bcrypt::hash("password", 4).unwrap();
        diesel::insert_into(crate::schema::users::table)
            .values(crate::models::NewUser {
                username: "Trusted",
                password_hash: &password_hash,
                role: "member",
                created_at: chrono::Utc::now().timestamp(),
                email: None,
            })
            .execute(&mut conn)
            .unwrap();
        let app = rocket::custom(rocket::Config::debug_default())
            .attach(wasd5::auth::sessions())
            .manage(pool.clone())
            .manage(wasd5::player_auth::PlayerLoginLimits::default())
            .mount("/", wasd5::player_auth::routes())
            .register("/api/player", wasd5::player_auth::catchers())
            .mount("/", routes())
            .mount(
                "/",
                rocket::routes![
                    crate::navigation_account,
                    crate::admin::admin_login_get,
                    crate::admin::admin_login_post,
                    crate::admin::admin_logout,
                    crate::admin::admin_index
                ],
            )
            .attach(Template::fairing());
        (dir, Client::tracked(app).unwrap(), pool)
    }

    fn player_login(client: &Client, username: &str) -> String {
        let response = client
            .post("/api/player/login")
            .remote("127.0.0.1:12345".parse().unwrap())
            .header(ContentType::JSON)
            .body(
                serde_json::json!({"username": username, "password": "password", "remember": true})
                    .to_string(),
            )
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
        assert_eq!(
            response.headers().get_one("Cache-Control"),
            Some("private, no-store")
        );
        let result: serde_json::Value = response.into_json().unwrap();
        result["token"].as_str().unwrap().to_owned()
    }

    fn assert_snapshot(client: &Client, pool: &DbPool, score: &serde_json::Value) {
        assert!(score["id"].as_i64().unwrap() > 0);
        let game = games::table
            .find(score["game_id"].as_str().unwrap())
            .first::<Game>(&mut pool.get().unwrap())
            .unwrap();
        let (mines, revealed, flags) = deserialize_state(&game).unwrap();
        let board = &score["board"];
        assert_eq!(board["rows"], game.rows);
        assert_eq!(board["columns"], game.board_columns);
        assert_eq!(board["cells"].as_array().unwrap().len(), mines.len());
        for (index, cell) in board["cells"].as_array().unwrap().iter().enumerate() {
            assert_eq!(cell["mine"], mines[index]);
            assert_eq!(cell["revealed"], revealed[index]);
            assert_eq!(cell["flagged"], flags[index]);
            assert_eq!(
                cell["count"],
                if mines[index] {
                    0
                } else {
                    adjacent_mines(
                        index,
                        &mines,
                        game.rows as usize,
                        game.board_columns as usize,
                    )
                }
            );
        }
        let page = client
            .get(format!("/games/minesweeper/scores/{}", score["id"]))
            .dispatch();
        assert_eq!(page.status(), Status::Ok);
        let html = page.into_string().unwrap();
        assert!(html.contains("Read-only snapshot"));
        assert!(html.contains("Back to Minesweeper"));
        assert!(html.contains("aria-label=\"Cell 1:"));
        assert_eq!(
            html.matches("class=\"minesweeper-cell").count(),
            mines.len()
        );
        let boards: serde_json::Value = client
            .get("/games/minesweeper/api/scores")
            .dispatch()
            .into_json()
            .unwrap();
        let entry = boards[game.mode]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["id"] == score["id"])
            .unwrap();
        assert_eq!(entry["board"], score["board"]);
        let html = client
            .get("/games/minesweeper")
            .dispatch()
            .into_string()
            .unwrap();
        assert!(html.contains("snapshot-miniature"));
        assert!(html.contains(&format!("/games/minesweeper/scores/{}", score["id"])));
    }

    #[test]
    fn makibishi_page_uses_website_identity_and_escapes_configuration() {
        let (_dir, client, pool) = client();
        let response = client.get("/games/makibishi").dispatch();
        assert_eq!(response.status(), Status::Ok);
        assert_eq!(response.headers().get_one("Cache-Control"), Some("private, no-store"));
        let html = response.into_string().unwrap();
        assert!(html.contains("id=\"makibishi-canvas\""));
        assert!(html.contains("id=\"makibishi-config\""));
        assert!(html.contains("data-signed-in=\"false\""));
        assert!(html.contains("data-username=\"\""));
        assert!(html.contains("/admin/login?next=makibishi"));
        assert!(html.contains("type=\"module\" src=\"/static/makibishi-loader.js\""));

        diesel::update(crate::schema::users::table)
            .set(crate::schema::users::display_name.eq(Some("\"<script>Visible & Player</script>")))
            .execute(&mut pool.get().unwrap())
            .unwrap();
        let response = client.post("/admin/login")
            .header(ContentType::Form)
            .body("username=Trusted&password=password&next=makibishi")
            .dispatch();
        assert_eq!(response.status(), Status::SeeOther);
        assert_eq!(response.headers().get_one("Location"), Some("/games/makibishi"));
        let html = client.get("/games/makibishi").dispatch().into_string().unwrap();
        assert!(html.contains("data-signed-in=\"true\""));
        assert!(html.contains("&quot;&lt;script&gt;Visible &amp; Player&lt;"));
        assert!(!html.contains("<script>Visible"));
        assert!(html.contains("/admin/logout"));
        assert!(!html.contains("password_hash"));
        assert!(!html.contains("Authorization"));
    }

    #[test]
    fn makibishi_login_return_is_allowlisted_for_members_and_gamers() {
        let (_dir, client, pool) = client();
        let html = client.get("/admin/login?next=makibishi")
            .dispatch().into_string().unwrap();
        assert!(html.contains("value=\"makibishi\""));
        let html = client.post("/admin/login")
            .header(ContentType::Form)
            .body("username=Trusted&password=wrong&next=makibishi")
            .dispatch().into_string().unwrap();
        assert!(html.contains("value=\"makibishi\""));
        diesel::update(crate::schema::users::table)
            .set(crate::schema::users::role.eq("gamer"))
            .execute(&mut pool.get().unwrap())
            .unwrap();
        for (next, expected) in [
            ("makibishi", "/games/makibishi"),
            ("https%3A%2F%2Fexample.com", "/games/minesweeper"),
        ] {
            let response = client.post("/admin/login")
                .header(ContentType::Form)
                .body(format!("username=Trusted&password=password&next={next}"))
                .dispatch();
            assert_eq!(response.status(), Status::SeeOther);
            assert_eq!(response.headers().get_one("Location"), Some(expected));
        }
    }

    #[test]
    fn browser_board_refresh_checks_website_ownership_and_expiry() {
        let (_dir, client, pool) = client();
        assert_eq!(client.post("/admin/login")
            .header(ContentType::Form)
            .body("username=Trusted&password=password")
            .dispatch().status(), Status::SeeOther);
        let html = client.get("/games/makibishi").dispatch().into_string().unwrap();
        let csrf = html.split("data-csrf=\"").nth(1).unwrap().split('"').next().unwrap();
        let view: serde_json::Value = client.post("/games/minesweeper/api/games")
            .header(ContentType::JSON)
            .body(serde_json::json!({"mode":"easy","csrf":csrf}).to_string())
            .dispatch().into_json().unwrap();
        let id = view["id"].as_str().unwrap();
        let url = format!("/games/minesweeper/api/games/{id}");
        let response = client.get(&url).dispatch();
        assert_eq!(response.status(), Status::Ok);
        assert_eq!(response.headers().get_one("Cache-Control"), Some("private, no-store"));
        assert_eq!(response.headers().get_one("Vary"), Some("Cookie"));
        assert_eq!(response.into_json::<serde_json::Value>().unwrap(), view);
        assert_eq!(client.post(format!("{url}/move"))
            .header(ContentType::JSON)
            .body(serde_json::json!({"action":"flag","row":0,"column":0,"csrf":csrf}).to_string())
            .dispatch().status(), Status::Ok);
        let refreshed: serde_json::Value = client.get(&url).dispatch().into_json().unwrap();
        assert_eq!(refreshed["cells"][0]["flagged"], true);
        assert_eq!(refreshed["clock_started"], true);
        assert!(refreshed["elapsed_ms"].as_i64().unwrap() >= 0);
        assert!(refreshed["cells"].as_array().unwrap().iter().all(|cell| cell["mine"] == false));
        let account: serde_json::Value = client.get("/api/navigation/account")
            .dispatch().into_json().unwrap();
        assert_eq!(account["username"], "Trusted");

        // A different account must not read the board, even with its UUID.
        diesel::update(games::table.find(id)).set(games::user_id.eq(None::<i32>))
            .execute(&mut pool.get().unwrap()).unwrap();
        assert_eq!(client.get(&url).dispatch().status(), Status::Forbidden);
        let owner = crate::schema::users::table.select(crate::schema::users::id)
            .first::<i32>(&mut pool.get().unwrap()).unwrap();
        diesel::update(games::table.find(id)).set(games::user_id.eq(Some(owner)))
            .execute(&mut pool.get().unwrap()).unwrap();
        client.get("/admin/logout").dispatch();
        assert_eq!(client.get(&url).dispatch().status(), Status::Forbidden);
        let account: serde_json::Value = client.get("/api/navigation/account")
            .dispatch().into_json().unwrap();
        assert!(account["username"].is_null());
        assert_eq!(client.post("/games/minesweeper/api/games")
            .header(ContentType::JSON)
            .body(serde_json::json!({"mode":"easy","csrf":csrf,"require_signed_in":true}).to_string())
            .dispatch().status(), Status::Unauthorized);
        assert_eq!(client.post("/games/minesweeper/api/games")
            .header(ContentType::JSON)
            .body(serde_json::json!({"mode":"easy","csrf":csrf}).to_string())
            .dispatch().status(), Status::Ok);
        diesel::update(games::table.find(id))
            .set(games::created_at.eq(chrono::Utc::now().timestamp_millis() - 86_400_001))
            .execute(&mut pool.get().unwrap()).unwrap();
        assert_eq!(client.get(&url).dispatch().status(), Status::NotFound);
        assert_eq!(client.get("/games/minesweeper/api/games/missing").dispatch().status(), Status::NotFound);
    }

    #[test]
    fn winning_board_migration_preserves_legacy_scores() {
        use diesel::connection::SimpleConnection;
        let mut conn = SqliteConnection::establish(":memory:").unwrap();
        conn.batch_execute(include_str!(
            "../migrations/00000000000001_create_users/up.sql"
        ))
        .unwrap();
        conn.batch_execute(include_str!(
            "../migrations/00000000000013_create_minesweeper/up.sql"
        ))
        .unwrap();
        conn.batch_execute("INSERT INTO minesweeper_scores (game_id, mode, player_name, elapsed_ms, created_at) VALUES ('old', 'easy', 'Legacy', 1000, 1)").unwrap();
        conn.batch_execute(include_str!(
            "../migrations/00000000000024_add_minesweeper_winning_board/up.sql"
        ))
        .unwrap();
        let score =
            Score::from_stored(scores::table.first::<StoredScore>(&mut conn).unwrap()).unwrap();
        assert_eq!(score.player_name, "Legacy");
        assert!(score.board.is_none());
        conn.batch_execute(include_str!(
            "../migrations/00000000000024_add_minesweeper_winning_board/down.sql"
        ))
        .unwrap();
        conn.batch_execute(include_str!(
            "../migrations/00000000000024_add_minesweeper_winning_board/up.sql"
        ))
        .unwrap();
        assert_eq!(
            scores::table.count().get_result::<i64>(&mut conn).unwrap(),
            1
        );
    }

    #[test]
    fn legacy_score_pages_are_public_and_names_are_escaped() {
        let (_dir, client, pool) = client();
        diesel::insert_into(scores::table)
            .values((
                scores::game_id.eq("legacy"),
                scores::mode.eq("easy"),
                scores::player_name.eq("<script>alert(1)</script>"),
                scores::elapsed_ms.eq(1000_i64),
                scores::created_at.eq(1_i64),
            ))
            .execute(&mut pool.get().unwrap())
            .unwrap();
        let score = scores::table
            .first::<StoredScore>(&mut pool.get().unwrap())
            .unwrap();
        let page = client
            .get(format!("/games/minesweeper/scores/{}", score.id))
            .dispatch();
        assert_eq!(page.status(), Status::Ok);
        let html = page.into_string().unwrap();
        assert!(html.contains("No board was saved"));
        assert!(!html.contains("<script>alert(1)</script>"));
        assert!(html.contains("&lt;script&gt;"));
        let boards: serde_json::Value = client
            .get("/games/minesweeper/api/scores")
            .dispatch()
            .into_json()
            .unwrap();
        assert_eq!(boards["easy"][0]["id"], score.id);
        assert!(boards["easy"][0]["board"].is_null());
        let html = client
            .get("/games/minesweeper")
            .dispatch()
            .into_string()
            .unwrap();
        assert!(html.contains("Board unavailable"));
        assert!(!html.contains("snapshot-miniature"));
        assert!(!html.contains("<script>alert(1)</script>"));
        assert_eq!(
            client
                .get("/games/minesweeper/scores/999999")
                .dispatch()
                .status(),
            Status::NotFound
        );
    }

    #[test]
    fn native_login_ranked_game_and_logout() {
        let (_dir, client, pool) = client();
        let token = player_login(&client, "Trusted");
        let auth = || rocket::http::Header::new("Authorization", format!("Bearer {token}"));
        let stored: String = wasd5::schema::player_sessions::table
            .select(wasd5::schema::player_sessions::token_hash)
            .first(&mut pool.get().unwrap())
            .unwrap();
        assert_ne!(stored, token);
        assert_eq!(stored.len(), 64);
        assert_eq!(
            client
                .post("/api/player/minesweeper/games")
                .header(ContentType::JSON)
                .body(r#"{"mode":"easy"}"#)
                .dispatch()
                .status(),
            Status::Unauthorized
        );
        // Native tokens do not authenticate browser/admin endpoints.
        let admin = client.get("/admin").header(auth()).dispatch();
        assert_eq!(admin.status(), Status::SeeOther);
        assert_eq!(admin.headers().get_one("Location"), Some("/admin/login"));
        assert_eq!(
            client
                .post("/games/minesweeper/api/games")
                .header(auth())
                .header(ContentType::JSON)
                .body(r#"{"mode":"easy"}"#)
                .dispatch()
                .status(),
            Status::Forbidden
        );
        let response = client
            .post("/api/player/minesweeper/games")
            .header(auth())
            .header(ContentType::JSON)
            .body(r#"{"mode":"easy"}"#)
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
        let mut view: serde_json::Value = response.into_json().unwrap();
        let id = view["id"].as_str().unwrap().to_owned();
        assert!(
            view["cells"]
                .as_array()
                .unwrap()
                .iter()
                .all(|c| c["mine"] == false && c["count"] == 0)
        );
        assert_eq!(
            client
                .post(format!("/api/player/minesweeper/games/{id}/score"))
                .header(auth())
                .header(ContentType::JSON)
                .body("{}")
                .dispatch()
                .status(),
            Status::UnprocessableEntity
        );
        let mines: Vec<bool> = serde_json::from_str(
            &games::table
                .find(&id)
                .select(games::mines)
                .first::<String>(&mut pool.get().unwrap())
                .unwrap(),
        )
        .unwrap();
        let mine = mines.iter().position(|&mine| mine).unwrap();
        assert_eq!(
            client
                .post(format!("/api/player/minesweeper/games/{id}/move"))
                .header(auth())
                .header(ContentType::JSON)
                .body(serde_json::json!({"action":"flag","row":mine/9,"column":mine%9}).to_string())
                .dispatch()
                .status(),
            Status::Ok
        );
        for (index, mine) in mines.iter().enumerate() {
            if *mine || view["cells"][index]["revealed"] == true || view["status"] == "won" {
                continue;
            }
            let response = client
                .post(format!("/api/player/minesweeper/games/{id}/move"))
                .header(auth())
                .header(ContentType::JSON)
                .body(
                    serde_json::json!({"action":"reveal","row":index/9,"column":index%9})
                        .to_string(),
                )
                .dispatch();
            assert_eq!(response.status(), Status::Ok);
            view = response.into_json().unwrap();
        }
        assert_eq!(view["status"], "won");
        let response = client
            .post(format!("/api/player/minesweeper/games/{id}/score"))
            .header(auth())
            .header(ContentType::JSON)
            .body(r#"{"player_name":"Imposter"}"#)
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
        let score: serde_json::Value = response.into_json().unwrap();
        assert_eq!(score["player_name"], "Trusted");
        assert!(score["user_id"].is_number());
        assert_eq!(score["elapsed_ms"], view["elapsed_ms"]);
        assert_snapshot(&client, &pool, &score);
        diesel::delete(games::table.find(&id))
            .execute(&mut pool.get().unwrap())
            .unwrap();
        let retry = client
            .post(format!("/api/player/minesweeper/games/{id}/score"))
            .header(auth())
            .header(ContentType::JSON)
            .body("{}")
            .dispatch();
        assert_eq!(retry.status(), Status::Ok);
        assert_eq!(retry.into_json::<serde_json::Value>().unwrap(), score);
        assert_eq!(
            client
                .get(format!("/games/minesweeper/scores/{}", score["id"]))
                .dispatch()
                .status(),
            Status::Ok
        );
        assert_eq!(
            scores::table
                .filter(scores::game_id.eq(&id))
                .count()
                .get_result::<i64>(&mut pool.get().unwrap())
                .unwrap(),
            1
        );
        assert_eq!(
            client
                .post("/api/player/logout")
                .header(auth())
                .dispatch()
                .status(),
            Status::Ok
        );
        assert_eq!(
            client
                .get("/api/player/me")
                .header(auth())
                .dispatch()
                .status(),
            Status::Unauthorized
        );
    }

    #[test]
    fn native_sessions_enforce_ownership_expiry_and_account_revocation() {
        let (_dir, client, pool) = client();
        let token = player_login(&client, "Trusted");
        let auth = || rocket::http::Header::new("Authorization", format!("Bearer {token}"));
        let response = client
            .post("/api/player/minesweeper/games")
            .header(auth())
            .header(ContentType::JSON)
            .body(r#"{"mode":"easy"}"#)
            .dispatch();
        let view: serde_json::Value = response.into_json().unwrap();
        let id = view["id"].as_str().unwrap();
        diesel::update(games::table.find(id))
            .set(games::user_id.eq(None::<i32>))
            .execute(&mut pool.get().unwrap())
            .unwrap();
        assert_eq!(
            client
                .get(format!("/api/player/minesweeper/games/{id}"))
                .header(auth())
                .dispatch()
                .status(),
            Status::Forbidden
        );
        assert_eq!(
            client
                .post(format!("/api/player/minesweeper/games/{id}/move"))
                .header(auth())
                .header(ContentType::JSON)
                .body(r#"{"action":"flag","row":0,"column":0}"#)
                .dispatch()
                .status(),
            Status::Forbidden
        );
        diesel::update(crate::schema::users::table)
            .set(crate::schema::users::disabled.eq(true))
            .execute(&mut pool.get().unwrap())
            .unwrap();
        assert_eq!(
            client
                .get("/api/player/me")
                .header(auth())
                .dispatch()
                .status(),
            Status::Unauthorized
        );
        diesel::update(crate::schema::users::table)
            .set((
                crate::schema::users::disabled.eq(false),
                crate::schema::users::session_version.eq(1),
            ))
            .execute(&mut pool.get().unwrap())
            .unwrap();
        assert_eq!(
            client
                .get("/api/player/me")
                .header(auth())
                .dispatch()
                .status(),
            Status::Unauthorized
        );
        let token = player_login(&client, "Trusted");
        diesel::update(wasd5::schema::player_sessions::table)
            .set(wasd5::schema::player_sessions::expires_at.eq(0))
            .execute(&mut pool.get().unwrap())
            .unwrap();
        assert_eq!(
            client
                .get("/api/player/me")
                .header(rocket::http::Header::new(
                    "Authorization",
                    format!("Bearer {token}")
                ))
                .dispatch()
                .status(),
            Status::Unauthorized
        );
    }

    #[test]
    fn native_login_rate_limits_invalid_credentials() {
        let (_dir, client, _pool) = client();
        for attempt in 0..6 {
            let response = client
                .post("/api/player/login")
                .remote("127.0.0.1:12345".parse().unwrap())
                .header(ContentType::JSON)
                .body(r#"{"username":"Trusted","password":"wrong"}"#)
                .dispatch();
            assert_eq!(
                response.status(),
                if attempt < 5 {
                    Status::Unauthorized
                } else {
                    Status::TooManyRequests
                }
            );
        }
    }

    #[test]
    fn server_created_board_is_hidden_and_guest_score_requires_a_win() {
        let (_dir, client, pool) = client();
        let page = client.get("/games/minesweeper").dispatch();
        assert_eq!(page.status(), Status::Ok);
        let html = page.into_string().unwrap();
        assert!(html.contains("Leaderboards"));
        let csrf = html
            .split("data-csrf=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();

        let started = client
            .post("/games/minesweeper/api/games")
            .header(ContentType::JSON)
            .body(serde_json::json!({"mode":"easy","csrf":csrf}).to_string())
            .dispatch();
        assert_eq!(started.status(), Status::Ok);
        let view: serde_json::Value = started.into_json().unwrap();
        let id = view["id"].as_str().unwrap();
        assert!(
            view["cells"]
                .as_array()
                .unwrap()
                .iter()
                .all(|cell| !cell["mine"].as_bool().unwrap())
        );
        assert_eq!(view["rows"], 9);
        assert_eq!(view["columns"], 9);
        assert_eq!(
            games::table
                .find(&id)
                .select(games::started_at)
                .first::<i64>(&mut pool.get().unwrap())
                .unwrap(),
            0
        );

        let mines: Vec<bool> = serde_json::from_str(
            &games::table
                .find(&id)
                .select(games::mines)
                .first::<String>(&mut pool.get().unwrap())
                .unwrap(),
        )
        .unwrap();
        let first_mine = mines.iter().position(|&mine| mine).unwrap();
        let first_move = client
            .post(format!("/games/minesweeper/api/games/{id}/move"))
            .header(ContentType::JSON)
            .body(
                serde_json::json!({
                    "action":"reveal",
                    "row":first_mine / 9,
                    "column":first_mine % 9,
                    "csrf":csrf
                })
                .to_string(),
            )
            .dispatch();
        assert_eq!(first_move.status(), Status::Ok);
        let first_view: serde_json::Value = first_move.into_json().unwrap();
        assert_eq!(first_view["status"], "playing");
        assert_eq!(first_view["clock_started"], true);
        assert!(
            games::table
                .find(&id)
                .select(games::started_at)
                .first::<i64>(&mut pool.get().unwrap())
                .unwrap()
                > 0
        );
        assert!(
            first_view["cells"][first_mine]["revealed"]
                .as_bool()
                .unwrap()
        );
        assert!(!first_view["cells"][first_mine]["mine"].as_bool().unwrap());

        let mines: Vec<bool> = serde_json::from_str(
            &games::table
                .find(&id)
                .select(games::mines)
                .first::<String>(&mut pool.get().unwrap())
                .unwrap(),
        )
        .unwrap();
        let mut final_view = first_view;
        for (index, mine) in mines.iter().enumerate() {
            if *mine || final_view["status"] != "playing" {
                continue;
            }
            if final_view["cells"][index]["revealed"] == true {
                continue;
            }
            let response = client
                .post(format!("/games/minesweeper/api/games/{id}/move"))
                .header(ContentType::JSON)
                .body(
                    serde_json::json!({
                        "action":"reveal",
                        "row":index / 9,
                        "column":index % 9,
                        "csrf":csrf
                    })
                    .to_string(),
                )
                .dispatch();
            assert_eq!(response.status(), Status::Ok);
            final_view = response.into_json().unwrap();
        }
        assert_eq!(final_view["status"], "won");
        assert!(final_view["elapsed_ms"].as_i64().unwrap() >= 0);

        let score = client
            .post(format!("/games/minesweeper/api/games/{id}/score"))
            .header(ContentType::JSON)
            .body(serde_json::json!({"player_name":"Guest","csrf":csrf}).to_string())
            .dispatch();
        assert_eq!(score.status(), Status::Ok);
        let score: serde_json::Value = score.into_json().unwrap();
        assert_eq!(score["player_name"], "Guest");
        assert!(score["user_id"].is_null());
        assert_snapshot(&client, &pool, &score);
        diesel::update(games::table.find(&id))
            .set(games::mines.eq(serde_json::to_string(&vec![false; 81]).unwrap()))
            .execute(&mut pool.get().unwrap())
            .unwrap();
        diesel::update(
            crate::schema::users::table.filter(crate::schema::users::username.eq("Trusted")),
        )
        .set(crate::schema::users::role.eq("gamer"))
        .execute(&mut pool.get().unwrap())
        .unwrap();

        let duplicate = client
            .post(format!("/games/minesweeper/api/games/{id}/score"))
            .header(ContentType::JSON)
            .body(serde_json::json!({"player_name":"Imposter","csrf":csrf}).to_string())
            .dispatch();
        assert_eq!(duplicate.status(), Status::Ok);
        assert_eq!(duplicate.into_json::<serde_json::Value>().unwrap(), score);

        assert_eq!(
            client
                .post("/admin/login")
                .header(ContentType::Form)
                .body("username=Trusted&password=password")
                .dispatch()
                .status(),
            Status::SeeOther
        );
        let signed_page = client
            .get("/games/minesweeper")
            .dispatch()
            .into_string()
            .unwrap();
        assert!(signed_page.contains("data-username=\"Trusted\""));
        diesel::update(
            crate::schema::users::table.filter(crate::schema::users::username.eq("Trusted")),
        )
        .set(crate::schema::users::display_name.eq(Some("Visible Player")))
        .execute(&mut pool.get().unwrap())
        .unwrap();
        let csrf = signed_page
            .split("data-csrf=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let started = client
            .post("/games/minesweeper/api/games")
            .header(ContentType::JSON)
            .body(serde_json::json!({"mode":"easy","csrf":csrf}).to_string())
            .dispatch();
        assert_eq!(started.status(), Status::Ok);
        let started: serde_json::Value = started.into_json().unwrap();
        let id = started["id"].as_str().unwrap().to_owned();
        let mines: Vec<bool> = serde_json::from_str(
            &games::table
                .find(&id)
                .select(games::mines)
                .first::<String>(&mut pool.get().unwrap())
                .unwrap(),
        )
        .unwrap();
        let mut final_view = started;
        for (index, mine) in mines.iter().enumerate() {
            if *mine || final_view["status"] != "playing" {
                continue;
            }
            if final_view["cells"][index]["revealed"] == true {
                continue;
            }
            let response = client
                .post(format!("/games/minesweeper/api/games/{id}/move"))
                .header(ContentType::JSON)
                .body(
                    serde_json::json!({
                        "action":"reveal",
                        "row":index / 9,
                        "column":index % 9,
                        "csrf":csrf
                    })
                    .to_string(),
                )
                .dispatch();
            assert_eq!(response.status(), Status::Ok);
            final_view = response.into_json().unwrap();
        }
        assert_eq!(final_view["status"], "won");
        let score = client
            .post(format!("/games/minesweeper/api/games/{id}/score"))
            .header(ContentType::JSON)
            .body(serde_json::json!({"player_name":"spoofed guest","csrf":csrf}).to_string())
            .dispatch();
        assert_eq!(score.status(), Status::Ok);
        let score: serde_json::Value = score.into_json().unwrap();
        assert_eq!(score["player_name"], "Visible Player");
        assert_eq!(
            score["user_id"],
            crate::schema::users::table
                .filter(crate::schema::users::username.eq("Trusted"))
                .select(crate::schema::users::id)
                .first::<i32>(&mut pool.get().unwrap())
                .unwrap()
        );
        diesel::update(
            crate::schema::users::table.filter(crate::schema::users::username.eq("Trusted")),
        )
        .set(crate::schema::users::display_name.eq(Some("Renamed Player")))
        .execute(&mut pool.get().unwrap())
        .unwrap();
        let boards: serde_json::Value = client
            .get("/games/minesweeper/api/scores")
            .dispatch()
            .into_json()
            .unwrap();
        let account_score = boards["easy"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| !entry["user_id"].is_null())
            .unwrap();
        assert_eq!(account_score["player_name"], "Renamed Player");
        assert!(
            boards["easy"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["player_name"] == "Guest")
        );
        let page = client
            .get("/games/minesweeper")
            .dispatch()
            .into_string()
            .unwrap();
        assert!(page.contains("title=\"Verified account\">✅"));
        assert!(page.contains("title=\"Anonymous player\">👻"));
    }
}
