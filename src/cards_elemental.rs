use diesel::prelude::*;
use rocket::http::{Cookie, CookieJar, Status};
use rocket::serde::json::Json;
use rocket::{State, get, post};
use rocket_dyn_templates::{Template, context};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use wasd5::cards_elemental_core::{
    Card, Opponent, Side, bot_card_index, full_deck, has_won, next_random, refill_hands, shuffle,
    winner_of,
};
#[cfg(test)]
use wasd5::cards_elemental_core::{Colour, Element, pro_card_index};

use crate::comics::Private;
use crate::db::DbPool;
use crate::schema::{
    cards_elemental_games as games, cards_elemental_online_games as online_games,
    cards_elemental_stats as stats,
};

type GameError = (Status, String);
type GameResult<T> = Result<T, GameError>;
const GAME_COOKIE: &str = "cards_elemental_game";
const ONLINE_COOKIE: &str = "cards_elemental_online_game";
const GAME_TTL_MS: i64 = 86_400_000;

pub fn routes() -> Vec<rocket::Route> {
    rocket::routes![
        index,
        start,
        current_game,
        play_card,
        create_online_game,
        open_online_games,
        join_online_game,
        current_online_game,
        play_online_card,
        game_stats
    ]
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum GameStatus {
    Waiting,
    Playing,
    Won,
    Lost,
    Draw,
}

#[derive(Clone, Deserialize, Serialize)]
struct RoundResult {
    round: u32,
    you_played: Card,
    server_played: Card,
    winner: Option<Side>,
}

#[derive(Deserialize, Serialize)]
struct GameState {
    #[serde(default)]
    opponent: Opponent,
    deck: Vec<Card>,
    #[serde(default)]
    discarded: Vec<Card>,
    your_hand: Vec<Card>,
    server_hand: Vec<Card>,
    your_winnings: Vec<Card>,
    server_winnings: Vec<Card>,
    starter: Side,
    pending_server_card: Option<Card>,
    round: u32,
    last_round: Option<RoundResult>,
    status: GameStatus,
    random_state: u64,
}

#[derive(Queryable)]
struct StoredGame {
    state: String,
}

#[derive(Insertable)]
#[diesel(table_name = games)]
struct NewGame<'a> {
    id: &'a str,
    state: &'a str,
    created_at: i64,
    updated_at: i64,
}

#[derive(Queryable)]
struct StoredOnlineGame {
    state: String,
}

#[derive(Insertable)]
#[diesel(table_name = online_games)]
struct NewOnlineGame<'a> {
    id: &'a str,
    visibility: &'a str,
    status: &'a str,
    state: &'a str,
    created_at: i64,
    updated_at: i64,
}

#[derive(Deserialize)]
struct StartRequest {
    csrf: String,
    #[serde(default)]
    opponent: Opponent,
}

#[derive(Deserialize)]
struct CreateOnlineRequest {
    visibility: String,
    csrf: String,
}

#[derive(Deserialize)]
struct JoinOnlineRequest {
    csrf: String,
}

#[derive(Deserialize)]
struct PlayRequest {
    card_index: usize,
    csrf: String,
}

#[derive(Serialize)]
struct GameView {
    mode: ViewMode,
    opponent: Option<Opponent>,
    your_hand: Vec<Card>,
    your_winnings: Vec<Card>,
    server_winnings: Vec<Card>,
    server_hand_count: usize,
    server_has_led: bool,
    your_card_pending: bool,
    your_played_card: Option<Card>,
    waiting_for_opponent: bool,
    deck_size: usize,
    game_id: Option<String>,
    starter: Side,
    round: u32,
    last_round: Option<RoundResult>,
    status: GameStatus,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum ViewMode {
    Solo,
    Online,
}

#[derive(Serialize)]
struct OpenOnlineGame {
    id: String,
}

#[derive(Deserialize, Serialize)]
struct OnlineGameState {
    deck: Vec<Card>,
    discarded: Vec<Card>,
    hands: [Vec<Card>; 2],
    winnings: [Vec<Card>; 2],
    player_tokens: [Option<String>; 2],
    starter: usize,
    pending: Option<OnlinePlayedCard>,
    round: u32,
    last_round: Option<OnlineRoundResult>,
    status: GameStatus,
    winner: Option<usize>,
    random_state: u64,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
struct OnlinePlayedCard {
    player: usize,
    card: Card,
}

#[derive(Clone, Deserialize, Serialize)]
struct OnlineRoundResult {
    round: u32,
    first_played: Card,
    second_played: Card,
    winner: Option<usize>,
}

#[derive(Serialize)]
struct OpenOnlineList {
    games: Vec<OpenOnlineGame>,
}

fn internal(error: impl std::fmt::Display) -> GameError {
    eprintln!("Cards Elemental operation failed: {error}");
    (
        Status::InternalServerError,
        "The game could not be loaded or saved. Please try again.".into(),
    )
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
            "Invalid form token. Reload the page and try again.".into(),
        ))
    }
}

fn new_state(id: &Uuid, opponent: Opponent) -> GameState {
    let mut deck = full_deck();
    let mut random_state = id
        .as_bytes()
        .iter()
        .fold(0x9e3779b97f4a7c15_u64, |seed, byte| {
            seed.rotate_left(7) ^ u64::from(*byte)
        });
    if random_state == 0 {
        random_state = 1;
    }
    shuffle(&mut deck, &mut random_state);
    let mut your_hand = Vec::with_capacity(5);
    let mut server_hand = Vec::with_capacity(5);
    for _ in 0..5 {
        your_hand.push(deck.pop().expect("a new deck has enough cards"));
        server_hand.push(deck.pop().expect("a new deck has enough cards"));
    }
    let starter = if next_random(&mut random_state) % 2 == 0 {
        Side::You
    } else {
        Side::Server
    };
    let mut state = GameState {
        opponent,
        deck,
        discarded: Vec::new(),
        your_hand,
        server_hand,
        your_winnings: Vec::new(),
        server_winnings: Vec::new(),
        starter,
        pending_server_card: None,
        round: 1,
        last_round: None,
        status: GameStatus::Playing,
        random_state,
    };
    if starter == Side::Server {
        state.pending_server_card = take_server_card(&mut state);
    }
    state
}

fn take_server_card(state: &mut GameState) -> Option<Card> {
    if state.server_hand.is_empty() {
        return None;
    }
    let index = bot_card_index(
        state.opponent,
        &state.server_hand,
        &state.server_winnings,
        &state.your_winnings,
        &state.discarded,
        &mut state.random_state,
    );
    Some(state.server_hand.remove(index))
}

fn view(state: &GameState) -> GameView {
    GameView {
        mode: ViewMode::Solo,
        opponent: Some(state.opponent),
        your_hand: state.your_hand.clone(),
        your_winnings: state.your_winnings.clone(),
        server_winnings: state.server_winnings.clone(),
        server_hand_count: state.server_hand.len()
            + usize::from(state.pending_server_card.is_some()),
        server_has_led: state.pending_server_card.is_some(),
        your_card_pending: false,
        your_played_card: None,
        waiting_for_opponent: false,
        deck_size: state.deck.len(),
        game_id: None,
        starter: state.starter,
        round: state.round,
        last_round: state.last_round.clone(),
        status: state.status,
    }
}

fn new_online_state(id: &Uuid, first_token: String) -> OnlineGameState {
    let solo_state = new_state(id, Opponent::Tosser);
    let mut hands = [solo_state.your_hand, solo_state.server_hand];
    if let Some(card) = solo_state.pending_server_card {
        hands[1].push(card);
    }
    OnlineGameState {
        deck: solo_state.deck,
        discarded: Vec::new(),
        hands,
        winnings: [Vec::new(), Vec::new()],
        player_tokens: [Some(first_token), None],
        starter: if solo_state.starter == Side::You {
            0
        } else {
            1
        },
        pending: None,
        round: 1,
        last_round: None,
        status: GameStatus::Waiting,
        winner: None,
        random_state: solo_state.random_state,
    }
}

fn online_view(state: &OnlineGameState, seat: usize, game_id: &str) -> GameView {
    let opponent = 1 - seat;
    let status = match state.status {
        GameStatus::Won if state.winner == Some(seat) => GameStatus::Won,
        GameStatus::Won => GameStatus::Lost,
        other => other,
    };
    let last_round = state.last_round.as_ref().map(|round| RoundResult {
        round: round.round,
        you_played: if seat == 0 {
            round.first_played
        } else {
            round.second_played
        },
        server_played: if seat == 0 {
            round.second_played
        } else {
            round.first_played
        },
        winner: round.winner.map(|winner| {
            if winner == seat {
                Side::You
            } else {
                Side::Server
            }
        }),
    });
    GameView {
        mode: ViewMode::Online,
        opponent: None,
        your_hand: state.hands[seat].clone(),
        your_winnings: state.winnings[seat].clone(),
        server_winnings: state.winnings[opponent].clone(),
        server_hand_count: state.hands[opponent].len()
            + usize::from(
                state
                    .pending
                    .is_some_and(|played| played.player == opponent),
            ),
        server_has_led: state
            .pending
            .is_some_and(|played| played.player == opponent),
        your_card_pending: state.pending.is_some_and(|played| played.player == seat),
        your_played_card: state
            .pending
            .filter(|played| played.player == seat)
            .map(|played| played.card),
        waiting_for_opponent: state.status == GameStatus::Waiting,
        deck_size: state.deck.len(),
        game_id: Some(game_id.to_owned()),
        starter: if state.starter == seat {
            Side::You
        } else {
            Side::Server
        },
        round: state.round,
        last_round,
        status,
    }
}

fn new_token() -> String {
    Uuid::new_v4().simple().to_string()
}

fn online_cookie_value(id: &str, seat: usize, token: &str) -> String {
    format!("{id}:{seat}:{token}")
}

fn parse_online_cookie(value: &str) -> Option<(String, usize, String)> {
    let mut parts = value.splitn(3, ':');
    let id = parts.next()?.to_owned();
    let seat = parts.next()?.parse::<usize>().ok()?;
    let token = parts.next()?.to_owned();
    if seat > 1 || Uuid::parse_str(&id).is_err() || token.is_empty() {
        return None;
    }
    Some((id, seat, token))
}

fn online_game(conn: &mut diesel::SqliteConnection, id: &str) -> GameResult<StoredOnlineGame> {
    let stored = online_games::table
        .find(id)
        .select((online_games::state, online_games::created_at))
        .first::<(String, i64)>(conn)
        .optional()
        .map_err(internal)?
        .ok_or((Status::NotFound, "Game not found or expired.".into()))?;
    if stored.1 < chrono::Utc::now().timestamp_millis() - GAME_TTL_MS {
        return Err((Status::NotFound, "Game not found or expired.".into()));
    }
    Ok(StoredOnlineGame { state: stored.0 })
}

fn read_online_state(game: &StoredOnlineGame) -> GameResult<OnlineGameState> {
    serde_json::from_str(&game.state).map_err(internal)
}

fn current_online_access(jar: &CookieJar<'_>) -> GameResult<Option<(String, usize, String)>> {
    match jar.get_private(ONLINE_COOKIE) {
        Some(cookie) => parse_online_cookie(cookie.value())
            .map(Some)
            .ok_or((Status::Unauthorized, "Invalid game session.".into())),
        None => Ok(None),
    }
}

fn verify_online_access(state: &OnlineGameState, seat: usize, token: &str) -> GameResult<()> {
    if state.player_tokens[seat].as_deref() == Some(token) {
        Ok(())
    } else {
        Err((
            Status::Forbidden,
            "You are not a player in this game.".into(),
        ))
    }
}

fn read_state(game: &StoredGame) -> GameResult<GameState> {
    serde_json::from_str(&game.state).map_err(internal)
}

fn current_id(jar: &CookieJar<'_>) -> GameResult<String> {
    jar.get_private(GAME_COOKIE)
        .map(|cookie| cookie.value().to_owned())
        .ok_or((Status::NotFound, "Start a new game first.".into()))
}

fn owned_game(conn: &mut diesel::SqliteConnection, id: &str) -> GameResult<StoredGame> {
    let (state, created_at) = games::table
        .find(id)
        .select((games::state, games::created_at))
        .first::<(String, i64)>(conn)
        .optional()
        .map_err(internal)?
        .ok_or((Status::NotFound, "Game not found or expired.".into()))?;
    if created_at < chrono::Utc::now().timestamp_millis() - GAME_TTL_MS {
        return Err((Status::NotFound, "Game not found or expired.".into()));
    }
    Ok(StoredGame { state })
}

#[derive(Queryable, Serialize)]
struct GameStat {
    category: String,
    completed: i64,
}

fn load_stats(conn: &mut diesel::SqliteConnection) -> GameResult<Vec<GameStat>> {
    stats::table
        .select((stats::category, stats::completed))
        .order(diesel::dsl::sql::<diesel::sql_types::Integer>(
            "CASE category WHEN 'pvp' THEN 0 WHEN 'private' THEN 1 WHEN 'tosser' THEN 2 WHEN 'pro' THEN 3 ELSE 4 END",
        ))
        .load(conn)
        .map_err(internal)
}

#[get("/games/cards-elemental/api/stats")]
fn game_stats(pool: &State<DbPool>) -> GameResult<Private<Json<Vec<GameStat>>>> {
    let mut conn = pool.get().map_err(internal)?;
    Ok(Private(Json(load_stats(&mut conn)?)))
}

#[get("/games/cards-elemental")]
fn index(
    navigation: crate::PageNavigation,
    jar: &CookieJar<'_>,
    pool: &State<DbPool>,
) -> GameResult<Private<Template>> {
    let mut conn = pool.get().map_err(internal)?;
    let statistics = load_stats(&mut conn)?;
    Ok(Private(Template::render(
        "games/cards_elemental",
        context! {
            pages: navigation.0,
            csrf: crate::ensure_csrf(jar),
            statistics,
        },
    )))
}

#[post(
    "/games/cards-elemental/api/games",
    format = "json",
    data = "<request>"
)]
fn start(
    pool: &State<DbPool>,
    jar: &CookieJar<'_>,
    request: Json<StartRequest>,
) -> GameResult<Private<Json<GameView>>> {
    csrf(jar, &request.csrf)?;
    let uuid = Uuid::new_v4();
    let id = uuid.as_hyphenated().to_string();
    let state = new_state(&uuid, request.opponent);
    let state_json = serde_json::to_string(&state).map_err(internal)?;
    let now = chrono::Utc::now().timestamp_millis();
    let mut conn = pool.get().map_err(internal)?;
    diesel::delete(games::table.filter(games::created_at.lt(now - GAME_TTL_MS)))
        .execute(&mut conn)
        .map_err(internal)?;
    diesel::insert_into(games::table)
        .values(NewGame {
            id: &id,
            state: &state_json,
            created_at: now,
            updated_at: now,
        })
        .execute(&mut conn)
        .map_err(internal)?;
    jar.remove_private(Cookie::new(ONLINE_COOKIE, ""));
    jar.add_private(Cookie::new(GAME_COOKIE, id));
    Ok(Private(Json(view(&state))))
}

#[get("/games/cards-elemental/api/game")]
fn current_game(
    pool: &State<DbPool>,
    jar: &CookieJar<'_>,
) -> GameResult<Private<Json<Option<GameView>>>> {
    let id = match current_id(jar) {
        Ok(id) => id,
        Err((status, _)) if status == Status::NotFound => return Ok(Private(Json(None))),
        Err(error) => return Err(error),
    };
    let mut conn = pool.get().map_err(internal)?;
    match owned_game(&mut conn, &id) {
        Ok(stored) => Ok(Private(Json(Some(view(&read_state(&stored)?))))),
        Err((status, _)) if status == Status::NotFound => {
            jar.remove_private(Cookie::new(GAME_COOKIE, ""));
            Ok(Private(Json(None)))
        }
        Err(error) => Err(error),
    }
}

#[post(
    "/games/cards-elemental/api/game/play",
    format = "json",
    data = "<request>"
)]
fn play_card(
    pool: &State<DbPool>,
    jar: &CookieJar<'_>,
    request: Json<PlayRequest>,
) -> GameResult<Private<Json<GameView>>> {
    csrf(jar, &request.csrf)?;
    let id = current_id(jar)?;
    let mut conn = pool.get().map_err(internal)?;
    let stored = owned_game(&mut conn, &id)?;
    let mut state = read_state(&stored)?;
    if state.status != GameStatus::Playing {
        return Err((Status::Conflict, "This game has already ended.".into()));
    }
    if request.card_index >= state.your_hand.len() {
        return Err((
            Status::UnprocessableEntity,
            "Choose a card from your hand.".into(),
        ));
    }
    let you_played = state.your_hand.remove(request.card_index);
    let server_played = match state.pending_server_card.take() {
        Some(card) => card,
        None => take_server_card(&mut state)
            .ok_or((Status::Conflict, "The server has no cards to play.".into()))?,
    };
    let winner = winner_of(you_played, server_played);
    match winner {
        Some(Side::You) => state.your_winnings.push(you_played),
        Some(Side::Server) => {
            state.server_winnings.push(server_played);
            state.discarded.push(you_played);
        }
        None => state.discarded.extend([you_played, server_played]),
    }
    if winner == Some(Side::You) {
        state.discarded.push(server_played);
    }
    state.last_round = Some(RoundResult {
        round: state.round,
        you_played,
        server_played,
        winner,
    });
    state.round += 1;
    if has_won(&state.your_winnings) {
        state.status = GameStatus::Won;
    } else if has_won(&state.server_winnings) {
        state.status = GameStatus::Lost;
    } else {
        refill_hands(
            &mut state.deck,
            &mut state.discarded,
            &mut state.your_hand,
            &mut state.server_hand,
            &mut state.random_state,
        );
        if state.your_hand.is_empty() || state.server_hand.is_empty() {
            state.status = GameStatus::Draw;
        } else {
            if let Some(winner) = winner {
                state.starter = winner;
            }
            if state.starter == Side::Server {
                state.pending_server_card = take_server_card(&mut state);
            }
        }
    }
    let new_state = serde_json::to_string(&state).map_err(internal)?;
    let now = chrono::Utc::now().timestamp_millis();
    let changed = diesel::update(
        games::table
            .filter(games::id.eq(&id))
            .filter(games::state.eq(&stored.state)),
    )
    .set((games::state.eq(new_state), games::updated_at.eq(now)))
    .execute(&mut conn)
    .map_err(internal)?;
    if changed == 0 {
        return Err((
            Status::Conflict,
            "The game changed before your card was played. Please try again.".into(),
        ));
    }
    Ok(Private(Json(view(&state))))
}

#[post(
    "/games/cards-elemental/api/online",
    format = "json",
    data = "<request>"
)]
fn create_online_game(
    pool: &State<DbPool>,
    jar: &CookieJar<'_>,
    request: Json<CreateOnlineRequest>,
) -> GameResult<Private<Json<GameView>>> {
    csrf(jar, &request.csrf)?;
    if request.visibility != "public" && request.visibility != "private" {
        return Err((
            Status::UnprocessableEntity,
            "Choose a public or private game.".into(),
        ));
    }
    let uuid = Uuid::new_v4();
    let id = uuid.as_hyphenated().to_string();
    let token = new_token();
    let state = new_online_state(&uuid, token.clone());
    let state_json = serde_json::to_string(&state).map_err(internal)?;
    let now = chrono::Utc::now().timestamp_millis();
    let mut conn = pool.get().map_err(internal)?;
    diesel::delete(online_games::table.filter(online_games::created_at.lt(now - GAME_TTL_MS)))
        .execute(&mut conn)
        .map_err(internal)?;
    diesel::insert_into(online_games::table)
        .values(NewOnlineGame {
            id: &id,
            visibility: &request.visibility,
            status: "waiting",
            state: &state_json,
            created_at: now,
            updated_at: now,
        })
        .execute(&mut conn)
        .map_err(internal)?;
    jar.remove_private(Cookie::new(GAME_COOKIE, ""));
    jar.add_private(Cookie::new(
        ONLINE_COOKIE,
        online_cookie_value(&id, 0, &token),
    ));
    Ok(Private(Json(online_view(&state, 0, &id))))
}

#[get("/games/cards-elemental/api/online/open")]
fn open_online_games(pool: &State<DbPool>) -> GameResult<Private<Json<OpenOnlineList>>> {
    let now = chrono::Utc::now().timestamp_millis();
    let mut conn = pool.get().map_err(internal)?;
    let rows = online_games::table
        .filter(online_games::visibility.eq("public"))
        .filter(online_games::status.eq("waiting"))
        .filter(online_games::created_at.ge(now - GAME_TTL_MS))
        .order(online_games::created_at.desc())
        .select(online_games::id)
        .limit(20)
        .load::<String>(&mut conn)
        .map_err(internal)?;
    Ok(Private(Json(OpenOnlineList {
        games: rows.into_iter().map(|id| OpenOnlineGame { id }).collect(),
    })))
}

#[post(
    "/games/cards-elemental/api/online/<id>/join",
    format = "json",
    data = "<request>"
)]
fn join_online_game(
    id: &str,
    pool: &State<DbPool>,
    jar: &CookieJar<'_>,
    request: Json<JoinOnlineRequest>,
) -> GameResult<Private<Json<GameView>>> {
    csrf(jar, &request.csrf)?;
    let id = Uuid::parse_str(id)
        .map_err(|_| (Status::NotFound, "Game not found or expired.".into()))?
        .as_hyphenated()
        .to_string();
    if current_online_access(jar)?.is_some_and(|(current_id, _, _)| current_id == id) {
        return Err((
            Status::Conflict,
            "You cannot join your own game from the same browser session.".into(),
        ));
    }
    let token = new_token();
    let mut conn = pool.get().map_err(internal)?;
    let stored = online_game(&mut conn, &id)?;
    let mut state = read_online_state(&stored)?;
    if state.status != GameStatus::Waiting || state.player_tokens[1].is_some() {
        return Err((
            Status::Conflict,
            "This game already has two players or has already started.".into(),
        ));
    }
    state.player_tokens[1] = Some(token.clone());
    state.status = GameStatus::Playing;
    let new_state = serde_json::to_string(&state).map_err(internal)?;
    let now = chrono::Utc::now().timestamp_millis();
    let changed = diesel::update(
        online_games::table
            .filter(online_games::id.eq(&id))
            .filter(online_games::status.eq("waiting"))
            .filter(online_games::state.eq(&stored.state)),
    )
    .set((
        online_games::state.eq(new_state),
        online_games::status.eq("playing"),
        online_games::updated_at.eq(now),
    ))
    .execute(&mut conn)
    .map_err(internal)?;
    if changed == 0 {
        return Err((
            Status::Conflict,
            "Someone else joined this game first. Please choose another.".into(),
        ));
    }
    jar.remove_private(Cookie::new(GAME_COOKIE, ""));
    jar.add_private(Cookie::new(
        ONLINE_COOKIE,
        online_cookie_value(&id, 1, &token),
    ));
    Ok(Private(Json(online_view(&state, 1, &id))))
}

#[get("/games/cards-elemental/api/online/current")]
fn current_online_game(
    pool: &State<DbPool>,
    jar: &CookieJar<'_>,
) -> GameResult<Private<Json<Option<GameView>>>> {
    let Some((id, seat, token)) = current_online_access(jar)? else {
        return Ok(Private(Json(None)));
    };
    let mut conn = pool.get().map_err(internal)?;
    match online_game(&mut conn, &id) {
        Ok(stored) => {
            let state = read_online_state(&stored)?;
            verify_online_access(&state, seat, &token)?;
            Ok(Private(Json(Some(online_view(&state, seat, &id)))))
        }
        Err((status, _)) if status == Status::NotFound => {
            jar.remove_private(Cookie::new(ONLINE_COOKIE, ""));
            Ok(Private(Json(None)))
        }
        Err(error) => Err(error),
    }
}

#[post(
    "/games/cards-elemental/api/online/play",
    format = "json",
    data = "<request>"
)]
fn play_online_card(
    pool: &State<DbPool>,
    jar: &CookieJar<'_>,
    request: Json<PlayRequest>,
) -> GameResult<Private<Json<GameView>>> {
    csrf(jar, &request.csrf)?;
    let (id, seat, token) = current_online_access(jar)?
        .ok_or((Status::NotFound, "Join an online game first.".into()))?;
    let mut conn = pool.get().map_err(internal)?;
    let stored = online_game(&mut conn, &id)?;
    let mut state = read_online_state(&stored)?;
    verify_online_access(&state, seat, &token)?;
    if state.status != GameStatus::Playing {
        return Err((
            Status::Conflict,
            "This game is not currently playing.".into(),
        ));
    }
    if request.card_index >= state.hands[seat].len() {
        return Err((
            Status::UnprocessableEntity,
            "Choose a card from your hand.".into(),
        ));
    }
    let played = state.hands[seat].remove(request.card_index);
    match state.pending.take() {
        None if seat == state.starter => {
            state.pending = Some(OnlinePlayedCard {
                player: seat,
                card: played,
            });
        }
        None => {
            return Err((
                Status::Conflict,
                "Wait for the other player to lead this round.".into(),
            ));
        }
        Some(first) if first.player == seat => {
            state.pending = Some(first);
            return Err((
                Status::Conflict,
                "Wait for the other player to respond to your card.".into(),
            ));
        }
        Some(first) => {
            let winner = match winner_of(first.card, played) {
                Some(Side::You) => Some(first.player),
                Some(Side::Server) => Some(seat),
                None => None,
            };
            if let Some(winner) = winner {
                state.winnings[winner].push(if winner == first.player {
                    first.card
                } else {
                    played
                });
                state.discarded.push(if winner == first.player {
                    played
                } else {
                    first.card
                });
            } else {
                state.discarded.extend([first.card, played]);
            }
            let (first_played, second_played) = if first.player == 0 {
                (first.card, played)
            } else {
                (played, first.card)
            };
            state.last_round = Some(OnlineRoundResult {
                round: state.round,
                first_played,
                second_played,
                winner,
            });
            state.round += 1;
            if has_won(&state.winnings[0]) || has_won(&state.winnings[1]) {
                state.status = GameStatus::Won;
                state.winner = if has_won(&state.winnings[0]) {
                    Some(0)
                } else {
                    Some(1)
                };
            } else {
                let [first_hand, second_hand] = &mut state.hands;
                refill_hands(
                    &mut state.deck,
                    &mut state.discarded,
                    first_hand,
                    second_hand,
                    &mut state.random_state,
                );
                if state.hands[0].is_empty() || state.hands[1].is_empty() {
                    state.status = GameStatus::Draw;
                } else if let Some(winner) = winner {
                    state.starter = winner;
                }
            }
        }
    }
    let new_state = serde_json::to_string(&state).map_err(internal)?;
    let now = chrono::Utc::now().timestamp_millis();
    let status = match state.status {
        GameStatus::Waiting => "waiting",
        GameStatus::Playing => "playing",
        GameStatus::Won | GameStatus::Lost => "won",
        GameStatus::Draw => "draw",
    };
    let changed = diesel::update(
        online_games::table
            .filter(online_games::id.eq(&id))
            .filter(online_games::state.eq(&stored.state)),
    )
    .set((
        online_games::state.eq(new_state),
        online_games::status.eq(status),
        online_games::updated_at.eq(now),
    ))
    .execute(&mut conn)
    .map_err(internal)?;
    if changed == 0 {
        return Err((
            Status::Conflict,
            "The game changed before your card was played. Please try again.".into(),
        ));
    }
    Ok(Private(Json(online_view(&state, seat, &id))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use rocket::http::ContentType;
    use rocket::local::blocking::Client;

    fn card(element: Element, colour: Colour, value: u8) -> Card {
        Card {
            element,
            colour,
            value,
        }
    }

    fn client() -> (Client, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let pool = db::establish_pool(dir.path().join("cards.db").to_str().unwrap());
        let mut conn = pool.get().unwrap();
        db::run_migrations(&mut conn).unwrap();
        let app = rocket::custom(rocket::Config::debug_default())
            .manage(pool)
            .mount("/", routes())
            .attach(Template::fairing());
        (Client::tracked(app).unwrap(), dir)
    }

    fn client_pair() -> (Client, Client, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let pool = db::establish_pool(dir.path().join("cards.db").to_str().unwrap());
        let mut conn = pool.get().unwrap();
        db::run_migrations(&mut conn).unwrap();
        let make_client = || {
            let app = rocket::custom(rocket::Config::debug_default())
                .manage(pool.clone())
                .mount("/", routes())
                .attach(Template::fairing());
            Client::tracked(app).unwrap()
        };
        (make_client(), make_client(), dir)
    }

    fn csrf_for(client: &Client) -> String {
        let html = client
            .get("/games/cards-elemental")
            .dispatch()
            .into_string()
            .unwrap();
        html.split("data-csrf=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn completed_counts_are_atomic_once_only_and_survive_session_cleanup() {
        let (client, _dir) = client();
        let pool = client.rocket().state::<DbPool>().unwrap();
        let mut conn = pool.get().unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        for opponent in ["tosser", "pro", "pro2"] {
            for status in ["won", "lost", "draw"] {
                let id = format!("{opponent}-{status}");
                let playing =
                    serde_json::json!({"status":"playing", "opponent":opponent}).to_string();
                diesel::insert_into(games::table)
                    .values(NewGame {
                        id: &id,
                        state: &playing,
                        created_at: now,
                        updated_at: now,
                    })
                    .execute(&mut conn)
                    .unwrap();
                let ended = serde_json::json!({"status":status, "opponent":opponent}).to_string();
                for _ in 0..2 {
                    diesel::update(games::table.find(&id))
                        .set(games::state.eq(&ended))
                        .execute(&mut conn)
                        .unwrap();
                }
            }
        }
        for visibility in ["public", "private"] {
            for outcome in ["won", "draw"] {
                let id = format!("{visibility}-{outcome}");
                diesel::insert_into(online_games::table)
                    .values(NewOnlineGame {
                        id: &id,
                        visibility,
                        status: "waiting",
                        state: "{}",
                        created_at: now,
                        updated_at: now,
                    })
                    .execute(&mut conn)
                    .unwrap();
                diesel::update(online_games::table.find(&id))
                    .set(online_games::status.eq("playing"))
                    .execute(&mut conn)
                    .unwrap();
                for _ in 0..2 {
                    diesel::update(online_games::table.find(&id))
                        .set(online_games::status.eq(outcome))
                        .execute(&mut conn)
                        .unwrap();
                }
            }
        }
        let expected = vec![
            ("pvp", 2),
            ("private", 2),
            ("tosser", 3),
            ("pro", 3),
            ("pro2", 3),
        ];
        let read = |conn: &mut diesel::SqliteConnection| {
            load_stats(conn)
                .unwrap()
                .into_iter()
                .map(|stat| (stat.category, stat.completed))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            read(&mut conn),
            expected
                .iter()
                .map(|(key, n)| (key.to_string(), *n))
                .collect::<Vec<_>>()
        );
        diesel::insert_into(games::table)
            .values(NewGame {
                id: "rolled-back",
                state: "{\"status\":\"playing\",\"opponent\":\"pro\"}",
                created_at: now,
                updated_at: now,
            })
            .execute(&mut conn)
            .unwrap();
        let rollback = conn.transaction::<(), diesel::result::Error, _>(|conn| {
            diesel::update(games::table.find("rolled-back"))
                .set(games::state.eq("{\"status\":\"won\",\"opponent\":\"pro\"}"))
                .execute(conn)?;
            assert_eq!(load_stats(conn).unwrap()[3].completed, 4);
            Err(diesel::result::Error::RollbackTransaction)
        });
        assert!(rollback.is_err());
        diesel::delete(games::table).execute(&mut conn).unwrap();
        diesel::delete(online_games::table)
            .execute(&mut conn)
            .unwrap();
        assert_eq!(
            read(&mut conn),
            expected
                .iter()
                .map(|(key, n)| (key.to_string(), *n))
                .collect::<Vec<_>>()
        );
        let statistics: serde_json::Value = client
            .get("/games/cards-elemental/api/stats")
            .dispatch()
            .into_json()
            .unwrap();
        assert_eq!(statistics[0]["category"], "pvp");
        assert_eq!(statistics[0]["completed"], 2);
        let html = client
            .get("/games/cards-elemental")
            .dispatch()
            .into_string()
            .unwrap();
        assert!(html.find("Completed games").unwrap() > html.find("How to play").unwrap());
        assert!(html.contains("id=\"cards-stat-pro\" class=\"mb-0\">3"));
        assert!(html.contains("id=\"cards-stat-pro2\" class=\"mb-0\">3"));
    }

    #[test]
    fn statistics_migration_backfills_existing_completed_sessions_only() {
        use diesel::connection::SimpleConnection;
        let (client, _dir) = client();
        let mut conn = client.rocket().state::<DbPool>().unwrap().get().unwrap();
        conn.batch_execute(include_str!(
            "../migrations/00000000000018_create_cards_elemental_stats/down.sql"
        ))
        .unwrap();
        conn.batch_execute(
            "INSERT INTO cards_elemental_games VALUES
              ('old', '{\"status\":\"won\"}', 0, 0),
              ('pro', '{\"status\":\"lost\",\"opponent\":\"pro\"}', 0, 0),
              ('pro2', '{\"status\":\"won\",\"opponent\":\"pro2\"}', 0, 0),
              ('active', '{\"status\":\"playing\"}', 0, 0);
             INSERT INTO cards_elemental_online_games VALUES
              ('public', 'public', 'won', '{}', 0, 0),
              ('private', 'private', 'draw', '{}', 0, 0),
              ('waiting', 'public', 'waiting', '{}', 0, 0);",
        )
        .unwrap();
        conn.batch_execute(include_str!(
            "../migrations/00000000000018_create_cards_elemental_stats/up.sql"
        ))
        .unwrap();
        conn.batch_execute(include_str!(
            "../migrations/00000000000019_add_cards_elemental_pro2_stats/up.sql"
        ))
        .unwrap();
        for stat in load_stats(&mut conn).unwrap() {
            assert_eq!(stat.completed, 1, "{}", stat.category);
        }
    }

    #[test]
    fn element_order_and_equal_element_values_determine_round_winner() {
        assert_eq!(
            winner_of(
                card(Element::Fire, Colour::Red, 1),
                card(Element::Ice, Colour::Red, 10)
            ),
            Some(Side::You)
        );
        assert_eq!(
            winner_of(
                card(Element::Ice, Colour::Red, 2),
                card(Element::Water, Colour::Blue, 1)
            ),
            Some(Side::You)
        );
        assert_eq!(
            winner_of(
                card(Element::Water, Colour::Red, 1),
                card(Element::Fire, Colour::Blue, 10)
            ),
            Some(Side::You)
        );
        assert_eq!(
            winner_of(
                card(Element::Water, Colour::Red, 4),
                card(Element::Water, Colour::Blue, 4)
            ),
            None
        );
        assert_eq!(
            winner_of(
                card(Element::Water, Colour::Red, 4),
                card(Element::Water, Colour::Blue, 5)
            ),
            Some(Side::Server)
        );
    }

    #[test]
    fn a_set_of_all_elements_or_three_matching_element_colour_cards_wins() {
        let three_elements = [
            card(Element::Fire, Colour::Red, 1),
            card(Element::Ice, Colour::Blue, 2),
            card(Element::Water, Colour::Yellow, 3),
        ];
        assert!(has_won(&three_elements));
        let three_matching = [
            card(Element::Fire, Colour::Red, 1),
            card(Element::Fire, Colour::Red, 2),
            card(Element::Fire, Colour::Red, 3),
        ];
        assert!(has_won(&three_matching));
        assert!(!has_won(&three_matching[..2]));
    }

    #[test]
    fn new_deck_has_120_cards_and_deals_five_to_each_player() {
        let state = new_state(&Uuid::new_v4(), Opponent::Tosser);
        assert_eq!(state.your_hand.len(), 5);
        assert_eq!(
            state.server_hand.len() + usize::from(state.pending_server_card.is_some()),
            5
        );
        assert_eq!(
            state.deck.len()
                + state.your_hand.len()
                + state.server_hand.len()
                + usize::from(state.pending_server_card.is_some())
                + state.your_winnings.len()
                + state.server_winnings.len(),
            120
        );
    }

    #[test]
    fn pro_targets_both_winning_sets_and_blocks_opponents_missing_element() {
        let fire = card(Element::Fire, Colour::Red, 10);
        let ice = card(Element::Ice, Colour::Blue, 10);
        let water = card(Element::Water, Colour::Green, 10);
        let hand = [fire, ice, water];
        let two_elements = [
            card(Element::Fire, Colour::Yellow, 1),
            card(Element::Ice, Colour::Green, 1),
        ];
        assert_eq!(pro_card_index(&hand, &two_elements, &[], &[]), 2);

        let matching_pair = [
            card(Element::Fire, Colour::Red, 1),
            card(Element::Fire, Colour::Red, 2),
        ];
        assert_eq!(pro_card_index(&hand, &matching_pair, &[], &[]), 0);
        assert_eq!(pro_card_index(&hand, &[], &two_elements, &[]), 1);
    }

    #[test]
    fn pro_choice_does_not_depend_on_hidden_cards_or_deck_order() {
        for opponent in [Opponent::Pro, Opponent::Pro2] {
            let mut state = new_state(&Uuid::from_u128(42), opponent);
            state.pending_server_card = None;
            let json = serde_json::to_string(&state).unwrap();
            let mut other: GameState = serde_json::from_str(&json).unwrap();
            other.your_hand.reverse();
            other.your_hand[0] = card(Element::Fire, Colour::Red, 1);
            other.deck.reverse();
            other.random_state = 98765;
            assert_eq!(take_server_card(&mut state), take_server_card(&mut other));
        }
    }

    #[test]
    fn pro_beats_random_play_more_often_than_it_loses() {
        let mut wins = 0;
        let mut losses = 0;
        for seed in 1..=600 {
            let mut state = new_state(&Uuid::from_u128(seed), Opponent::Pro);
            if let Some(pending) = state.pending_server_card.take() {
                state.server_hand.push(pending);
            }
            for _ in 0..200 {
                let index = next_random(&mut state.random_state) as usize % state.your_hand.len();
                let played = state.your_hand.remove(index);
                let server_played = take_server_card(&mut state).unwrap();
                match winner_of(played, server_played) {
                    Some(Side::You) => {
                        state.your_winnings.push(played);
                        state.discarded.push(server_played);
                    }
                    Some(Side::Server) => {
                        state.server_winnings.push(server_played);
                        state.discarded.push(played);
                    }
                    None => state.discarded.extend([played, server_played]),
                }
                if has_won(&state.your_winnings) {
                    losses += 1;
                    break;
                }
                if has_won(&state.server_winnings) {
                    wins += 1;
                    break;
                }
                refill_hands(
                    &mut state.deck,
                    &mut state.discarded,
                    &mut state.your_hand,
                    &mut state.server_hand,
                    &mut state.random_state,
                );
            }
        }
        assert_eq!(wins + losses, 600);
        eprintln!("Pro vs random: {wins} wins, {losses} losses");
        assert!(wins >= 360, "Pro should win at least 60%: {wins}/600");
    }

    #[test]
    fn solo_opponent_is_validated_persisted_and_defaults_to_tosser() {
        let (client, _dir) = client();
        let csrf = csrf_for(&client);
        for opponent in ["tosser", "pro", "pro2"] {
            let response = client
                .post("/games/cards-elemental/api/games")
                .header(ContentType::JSON)
                .body(serde_json::json!({"csrf":csrf, "opponent":opponent}).to_string())
                .dispatch();
            assert_eq!(response.status(), Status::Ok);
            let view: serde_json::Value = response.into_json().unwrap();
            assert_eq!(view["opponent"], opponent);
            let saved: serde_json::Value = client
                .get("/games/cards-elemental/api/game")
                .dispatch()
                .into_json()
                .unwrap();
            assert_eq!(saved["opponent"], opponent);
            let played = client
                .post("/games/cards-elemental/api/game/play")
                .header(ContentType::JSON)
                .body(serde_json::json!({"csrf":csrf, "card_index":0}).to_string())
                .dispatch();
            assert_eq!(played.status(), Status::Ok);
        }
        assert_eq!(
            client
                .post("/games/cards-elemental/api/games")
                .header(ContentType::JSON)
                .body(serde_json::json!({"csrf":csrf, "opponent":"invalid"}).to_string())
                .dispatch()
                .status(),
            Status::UnprocessableEntity
        );
        let state = new_state(&Uuid::from_u128(1), Opponent::Tosser);
        let mut json = serde_json::to_value(state).unwrap();
        json.as_object_mut().unwrap().remove("opponent");
        let legacy: GameState = serde_json::from_value(json).unwrap();
        assert_eq!(legacy.opponent, Opponent::Tosser);
    }

    #[test]
    fn discarded_cards_are_reshuffled_when_the_draw_pile_cannot_supply_both_players() {
        let mut deck = vec![card(Element::Fire, Colour::Red, 1)];
        let mut discarded = vec![
            card(Element::Ice, Colour::Blue, 2),
            card(Element::Water, Colour::Green, 3),
            card(Element::Fire, Colour::Yellow, 4),
        ];
        let mut first_hand = Vec::new();
        let mut second_hand = Vec::new();
        let mut random_state = 1;

        refill_hands(
            &mut deck,
            &mut discarded,
            &mut first_hand,
            &mut second_hand,
            &mut random_state,
        );

        assert_eq!(first_hand.len(), 1);
        assert_eq!(second_hand.len(), 1);
        assert_eq!(discarded.len(), 0);
        assert_eq!(
            deck.len() + discarded.len() + first_hand.len() + second_hand.len(),
            4
        );
    }

    #[test]
    fn game_api_hides_server_hand_and_resolves_one_card_per_round() {
        let (client, _dir) = client();
        let html = client
            .get("/games/cards-elemental")
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
        let start = client
            .post("/games/cards-elemental/api/games")
            .header(ContentType::JSON)
            .body(serde_json::json!({"csrf":csrf}).to_string())
            .dispatch();
        assert_eq!(start.status(), Status::Ok);
        let state: serde_json::Value = start.into_json().unwrap();
        assert_eq!(state["your_hand"].as_array().unwrap().len(), 5);
        assert_eq!(state["server_hand_count"], 5);
        assert_eq!(state["deck_size"], 110);
        assert_eq!(state["mode"], "solo");
        assert!(state.get("server_hand").is_none());
        assert!(state.get("deck").is_none());

        let current: serde_json::Value = client
            .get("/games/cards-elemental/api/game")
            .dispatch()
            .into_json()
            .unwrap();
        assert_eq!(current["your_hand"].as_array().unwrap().len(), 5);

        let played = client
            .post("/games/cards-elemental/api/game/play")
            .header(ContentType::JSON)
            .body(serde_json::json!({"card_index":0,"csrf":csrf}).to_string())
            .dispatch();
        assert_eq!(played.status(), Status::Ok);
        let state: serde_json::Value = played.into_json().unwrap();
        assert_eq!(state["round"], 2);
        assert_eq!(state["your_hand"].as_array().unwrap().len(), 5);
        assert_eq!(
            state["last_round"]["you_played"]["element"].is_string(),
            true
        );
        assert_eq!(
            state["last_round"]["server_played"]["element"].is_string(),
            true
        );
        assert_eq!(state["deck_size"], 108);
    }

    #[test]
    fn public_games_can_be_joined_and_players_only_see_their_own_hands() {
        let (host, guest, _dir) = client_pair();
        let host_csrf = csrf_for(&host);
        let guest_csrf = csrf_for(&guest);
        let created = host
            .post("/games/cards-elemental/api/online")
            .header(ContentType::JSON)
            .body(serde_json::json!({"visibility":"public","csrf":host_csrf}).to_string())
            .dispatch();
        assert_eq!(created.status(), Status::Ok);
        let host_state: serde_json::Value = created.into_json().unwrap();
        assert_eq!(host_state["mode"], "online");
        assert_eq!(host_state["status"], "waiting");
        assert_eq!(host_state["your_hand"].as_array().unwrap().len(), 5);
        let id = host_state["game_id"].as_str().unwrap();

        let listed: serde_json::Value = guest
            .get("/games/cards-elemental/api/online/open")
            .dispatch()
            .into_json()
            .unwrap();
        assert!(
            listed["games"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["id"] == id)
        );

        let joined = guest
            .post(format!("/games/cards-elemental/api/online/{id}/join"))
            .header(ContentType::JSON)
            .body(serde_json::json!({"csrf":guest_csrf}).to_string())
            .dispatch();
        assert_eq!(joined.status(), Status::Ok);
        let guest_state: serde_json::Value = joined.into_json().unwrap();
        assert_eq!(guest_state["status"], "playing");
        assert!(guest_state.get("hands").is_none());
        assert!(guest_state.get("player_tokens").is_none());
        assert!(guest_state.get("deck").is_none());

        let host_state: serde_json::Value = host
            .get("/games/cards-elemental/api/online/current")
            .dispatch()
            .into_json()
            .unwrap();
        assert_eq!(host_state["status"], "playing");
        assert_eq!(host_state["your_hand"].as_array().unwrap().len(), 5);
        assert_ne!(host_state["your_hand"], guest_state["your_hand"]);

        let (lead, lead_csrf, responder, responder_csrf) = if host_state["starter"] == "you" {
            (&host, &host_csrf, &guest, &guest_csrf)
        } else {
            (&guest, &guest_csrf, &host, &host_csrf)
        };
        let lead_result = lead
            .post("/games/cards-elemental/api/online/play")
            .header(ContentType::JSON)
            .body(serde_json::json!({"card_index":0,"csrf":lead_csrf}).to_string())
            .dispatch();
        assert_eq!(lead_result.status(), Status::Ok);
        let lead_state: serde_json::Value = lead_result.into_json().unwrap();
        assert_eq!(lead_state["your_card_pending"], true);
        assert!(lead_state["your_played_card"]["element"].is_string());
        let reloaded: serde_json::Value = lead
            .get("/games/cards-elemental/api/online/current")
            .dispatch()
            .into_json()
            .unwrap();
        assert_eq!(reloaded["your_played_card"], lead_state["your_played_card"]);

        let response_state: serde_json::Value = responder
            .get("/games/cards-elemental/api/online/current")
            .dispatch()
            .into_json()
            .unwrap();
        assert_eq!(response_state["server_has_led"], true);
        assert!(response_state["your_played_card"].is_null());
        assert_eq!(response_state["your_hand"].as_array().unwrap().len(), 5);
        let resolved = responder
            .post("/games/cards-elemental/api/online/play")
            .header(ContentType::JSON)
            .body(serde_json::json!({"card_index":0,"csrf":responder_csrf}).to_string())
            .dispatch();
        assert_eq!(resolved.status(), Status::Ok);
        let resolved_state: serde_json::Value = resolved.into_json().unwrap();
        assert_eq!(resolved_state["round"], 2);
        assert_eq!(resolved_state["last_round"]["round"], 1);
        assert!(resolved_state["your_played_card"].is_null());
    }

    #[test]
    fn private_games_are_not_listed_but_can_be_joined_by_id() {
        let (host, guest, _dir) = client_pair();
        let host_csrf = csrf_for(&host);
        let guest_csrf = csrf_for(&guest);
        let created = host
            .post("/games/cards-elemental/api/online")
            .header(ContentType::JSON)
            .body(serde_json::json!({"visibility":"private","csrf":host_csrf}).to_string())
            .dispatch();
        assert_eq!(created.status(), Status::Ok);
        let state: serde_json::Value = created.into_json().unwrap();
        let id = state["game_id"].as_str().unwrap();
        let listed: serde_json::Value = guest
            .get("/games/cards-elemental/api/online/open")
            .dispatch()
            .into_json()
            .unwrap();
        assert!(
            !listed["games"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["id"] == id)
        );

        let joined = guest
            .post(format!("/games/cards-elemental/api/online/{id}/join"))
            .header(ContentType::JSON)
            .body(serde_json::json!({"csrf":guest_csrf}).to_string())
            .dispatch();
        assert_eq!(joined.status(), Status::Ok);
    }
}
