# wasd5

A Rocket.rs based webapp for my personal site.

## Games

Minesweeper is available at `/games/minesweeper` in Easy, Intermediate,
Advanced, and Custom modes. The server generates boards, applies moves, and
measures leaderboard times; the three fixed modes have separate leaderboards.
Signed-in users appear under their account name with an **Account** badge.
Guests choose a name after winning and are marked as unverified. Server-side
validation makes scores harder to forge, but does not prevent automated play.

Tetris is available at `/games/tetris`. Use H/J/K/L or the arrow keys to move
and soft-drop; K/Up rotates counter-clockwise, I/Enter clockwise, and Space
hard-drops. Tetris scores are submitted once per server-issued game run, but
gameplay is client-side and scores are not independently replay-validated.

Cards Elemental is available at `/games/cards-elemental` as a solo game against
the server or as an online two-player game. Choose **Tosser** for random solo
play, **Pro** for a strategic opponent, or **Pro2** for stronger colour-pair
goals and defence against imminent wins via either collection rule. Pro is
unchanged; Pro2 activates its stronger colour preference only after its won
pile contains a matching element/colour pair, not while considering a
hypothetical pair. Before that it uses Pro's collection goals. It penalizes an
opponent's game-winning capture more heavily than speculative progress.
Both use only their own hand and public collections/discards, without inspecting
hidden hands, pending cards, or deck order. Public games appear in the open
games list; private games can be joined using their shareable game ID. Online
games synchronize through server-side state and expire after 24 hours. The
draw-pile count is shown in-game; when it cannot supply both players, discarded
cards are shuffled back in while won cards stay out of play. It follows the
elemental card rules from the
[`cards_elemental`](https://github.com/gugi9000/cards_elemental) reference
project. There is no leaderboard. Below the rules, persistent backend totals
show completed public PvP, private PvP, Solo Tosser, Solo Pro, and Solo Pro2 games.
Only a winner or draw counts; abandoned games do not. Totals survive session
expiry. The migration includes completed sessions still present in the database;
previously expired sessions cannot be recovered.

`/games/cards-elemental/stats` shows Humans vs Bots results for Tosser, Pro,
and Pro2: game wins/draws, rounds played (total/average/shortest/longest),
round results, and winning conditions. Completed solo and PvP games retain
permanent result summaries atomically with the final move, without hands or
player identifiers. PvP is not displayed on this page. Existing retained
completed sessions are backfilled; older counts without recoverable results
are explicitly marked unrecorded. Simulator games never enter these totals.

New round results play an effect for fire melting ice, water quenching fire,
ice freezing water, bigger-value wins, or ties. Effects play in solo and online
games, but not when restoring a saved result or polling it again. Browser
audio restrictions or playback failures are reported on the game page.

### Cards Elemental bot simulator

Run `cargo run --release --bin cards_elemental_sim` for 10,000 Tosser vs Pro
games. It uses the website's shared card rules and bot strategies, without
accessing the database or changing website statistics.

```powershell
cargo run --release --bin cards_elemental_sim -- --games 20000 --player1 tosser --player2 pro --seed 42
cargo run --release --bin cards_elemental_sim -- --player1 pro --player2 pro
cargo run --release --bin cards_elemental_sim -- --player1 pro2 --player2 pro --seed 42
```

The report includes game wins/losses/draws, win rates, total/average/minimum/
maximum rounds, round wins/losses/draws, and wins by either collection rule
(or both, counted separately). The opening player is randomized for each game.
A printed seed allows replaying the same batch. `--max-rounds` defaults to
10,000 per game; games reaching it are explicitly unfinished, not draws.
`--help` lists options and available bots. New bots belong in the shared
`Opponent` enum and `bot_card_index` dispatcher, making them available to
the simulator's player options too.

## Management TUI

`manage` is a Ratatui terminal application for trusted local operators. It opens
the same SQLite database as the website; it does not require an in-app login.
Anyone who can run it against the database has full administrative access.
Back up the database before upgrading; embedded migrations run on startup.
Deploy/restart the updated website as well as `manage` so account disabling and
session revocation are enforced by the running web process.

```powershell
$env:DATABASE_URL = 'wasd5.db'
$env:EDITOR = 'code --wait'
$env:WASD_PAGE_EDITOR = 'Your editing name'
cargo run --bin manage
```

Or run the built `manage` executable in an interactive terminal. Use
`manage --database <path> --editor-name <name>` to override the database and
page attribution. Without an attribution name the TUI prompts for one.
The system editor uses `VISUAL`, then `EDITOR`; use an editor that waits until
editing is complete. Quote executable paths containing spaces. No database
connection or write transaction is held while the editor is open.

Sections:

- **Users:** create/edit accounts, reset passwords, change roles/email and
  creatine reminder settings, disable or re-enable. Passwords are masked and
  never shown in details. Blank passwords keep the existing hash when editing.
  `d` disables rather than deleting; authored content and logs remain intact.
  Edit `Disabled` to false to restore access. Disabling, re-enabling, changing
  roles, or resetting a password revokes old sessions; users must log in again.
  The last active admin cannot be disabled or demoted. Disabled users cannot
  log in and receive no scheduled reminder emails.
- **Pages:** list public pages and hidden drafts, create pages, edit Markdown
  through the system editor, hide/publish, inspect full revision history.
  New pages default to hidden. Edits use the same revision and attribution
  mechanism as browser/CLI edits; existing slugs cannot be renamed here.
- **Calendar:** create/edit/delete appointments, manage people and ordering,
  and manage allowed IP addresses. Appointment forms refer to person IDs from
  the people list. Dates use `YYYY-MM-DD`; optional times use `HH:MM`. People
  with appointments cannot be deleted until those appointments are reassigned
  or removed.
- **Fitness timers:** create/edit/delete public workouts and reassign owners.
  Enter on Workout JSON opens the system editor. It uses the same validated
  `repeats` / `blocks` / `steps` definition as the web builder; each step has
  `label`, `seconds` (`null` for manual rest), and `repeats`.
- **Comics:** create/edit albums and edit existing strip captions, visibility,
  and ordering. `d` hides albums/strips instead of deleting their files. Uploads
  and banners remain in the website; strips cannot move between albums because
  image storage is per-album.
- **Packages:** create/edit/delete packages for any user, tracking numbers,
  ordered dates, and received dates (blank means pending).
- **Creatine logs:** create/edit/delete dated intake records for any user.

Keys: Left/Right or Tab switches sections; Up/Down (or j/k) selects records;
`n` creates; Enter/`e` edits; `d` disables/hides/deletes with confirmation;
`v` shows details (including page history); `/` filters by ID or summary;
`r` reloads; `q` quits. In forms, Tab/Up/Down selects fields, Enter/Space toggles
booleans, Enter opens Markdown/JSON in the system editor, Ctrl+S saves, and Esc
cancels. Unsaved changes require discard confirmation. Details support
Up/Down/PgUp/PgDn scrolling.

Saves and destructive actions check for concurrent changes rather than
overwriting stale records. Failed saves remain in the form. Editor failures
and failed saves preserve edited temporary files and display recovery paths.
Successful saves and explicitly discarded drafts clean up their temporary
files. Use `cargo test --test management --bin manage` for management tests.

### Remote management

Run the same TUI against a running website:

```powershell
manage --remote https://wasd.dk
```

It prompts for an **active admin** username and a masked password. All sections,
forms, revision/conflict checks, and local system-editor integration remain
available. Remote mode never opens a local database; `--database` and
`--editor-name` cannot be combined with `--remote`. Remote page attribution is
always the signed-in username, enforced by the server.

Remote access is **disabled by default**, independently of calendar access.
Configure exact client IP addresses on the **server**, then restart the updated
website:

```powershell
$env:MANAGEMENT_ALLOWED_IPS = '203.0.113.10,2001:db8::10'
$env:MANAGEMENT_TRUSTED_PROXIES = '127.0.0.1,::1'
```

For a server managed by **Supervisor**, add the variables to the existing
program configuration; switching to systemd is not necessary:

```ini
[program:wasd5]
; Keep your existing command, directory, and other settings.
environment=MANAGEMENT_ALLOWED_IPS="203.0.113.10",MANAGEMENT_TRUSTED_PROXIES="127.0.0.1,::1"
```

Replace `203.0.113.10` with your **client's public IP**, not the server's IP.
Set the trusted proxy addresses to the actual reverse-proxy peers. Preserve
any existing entries on the `environment=` line when adding these variables.
Apply the configuration changes:

```bash
sudo supervisorctl reread
sudo supervisorctl update
```

The app does not load `.env` files automatically; variables must be supplied by
the process environment or deployment tooling. Systemd also supports
`Environment=` and `EnvironmentFile=`, but is not required for this setup.

Both settings accept comma/whitespace-separated IPs, not CIDR ranges. Invalid
settings stop startup rather than silently broadening access.
`MANAGEMENT_TRUSTED_PROXIES` must contain only the actual network peers of your
TLS-terminating reverse proxy. The management API uses the socket peer, **not**
Rocket's configurable IP header, to decide whether to trust forwarded headers.
Only explicitly trusted proxies may supply `X-Real-IP` and
`X-Forwarded-Proto: https`; absent, repeated, malformed, or non-HTTPS headers are
rejected. The proxy must **overwrite** these headers, never pass client-supplied
values through. For example, inside your HTTPS nginx server:

```nginx
location /api/manage/ {
    proxy_pass http://127.0.0.1:5010;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-Proto $scheme;
}
```

This Rocket build has no direct TLS listener, so non-loopback management
connections must arrive through such a trusted TLS proxy. Keep the Rocket
listener private/firewalled. Allowlisting a proxy's address alone does **not**
allow all of its clients when it is configured as a trusted proxy.

HTTPS with normal certificate validation is mandatory in the client, except
for direct localhost development:

```powershell
# Development server only (do not use behind a public, unconfigured proxy):
$env:MANAGEMENT_ALLOWED_IPS = '127.0.0.1,::1'
# MANAGEMENT_TRUSTED_PROXIES must be unset for direct localhost HTTP.
manage --remote http://127.0.0.1:5010
```

The client refuses redirects, embedded credentials, URL paths/queries, and
non-local HTTP. It does not expose a certificate-verification bypass.
The `/api/manage/*` endpoints require a dedicated bearer session, not a browser
cookie, and all responses are non-cacheable. Five failed login attempts per
client IP trigger a five-minute login limit. Sessions are held only in server
and client memory, bound to the login IP, expire after one hour, and are
invalidated by logout, server restart, account disabling, role changes, or
password resets. Admin status and account/session version are checked on each
request and again inside write transactions. If access expires, failed saves
keep the form and editor recovery files; restart `manage` to log in again.

Validate with `cargo test --test remote_management --lib --bin manage`.

## Comics

The top-level Comics link opens `/comics`. Any signed-in user can create albums
and upload strips/pages. Accounts are created with the existing admin user tools.
The album owner and admins can edit titles, optional plain-text subtexts,
visibility, and strip ordering. New forms default to hidden drafts.

Albums can optionally have banner art, shown above the album and on its gallery
card. Create the album, then use Edit album to upload, replace, or remove its
banner. Banners use the same validated image formats, size limits, private
per-album storage, and visibility checks as strips. Replacement/removal deletes
the old file after the metadata change commits; failed saves retain the existing
banner and clean up the new upload.

A hidden album hides all its strips and images. A published album can contain
individually hidden strips. Owners and admins can preview their drafts; everyone
else receives a 404 for hidden albums/strips, including direct image URLs.
Authenticated gallery responses and image responses are marked private/no-store
so publishing and hiding do not leave publicly cacheable copies. Previously
downloaded images cannot be revoked from a visitor's device.

Metadata is stored in SQLite; images are stored outside `static`, in
`uploads/comics/<album-id>/<generated-name>.<extension>` by default. Set
`COMICS_DIR` to move this private storage directory (it must not be under
`static`). Never expose it through a web-server alias or public file mount.
The app's `/comics/<album-id>/strips/<strip-id>/image` route checks both album and
strip visibility before serving an image.

Uploads accept validated PNG, JPEG, GIF, or WebP files up to 20 MiB, with
dimension and decoder memory limits. SVG is not supported. Images keep their
original encoding; filenames and extensions are generated from the detected
image format rather than the uploaded name. Strip metadata is committed only
after the file has been copied; failed SQL writes clean up the new file.
A process crash between the filesystem write and SQL commit can leave an
unreferenced file, but cannot publish an image without its metadata.

Schema migrations run automatically on startup. Mount persistent storage for
both `DATABASE_URL` and `COMICS_DIR` in production and back them up together
while uploads are stopped. Deploying only the executable/database is not enough
to preserve images.

## Fitness timers

The Health menu's Fitness timers link opens the public library at `/health/timer`.
Signed-in users can build and save public workouts; anyone can run them without
an account. Share a workout's `/health/timer/<id>` link. Only its owner or an
admin can edit or delete it. Accounts use the existing admin user tools.
Timers are stored in SQLite and the migration runs automatically at startup.

The builder includes EMOM X (X one-minute intervals), Tabata (20 seconds work /
10 seconds rest, initially 8 rounds), and Custom presets. Applying a preset
replaces the current steps. Custom workouts support timed steps, manual rest
steps, step repeats, repeatable groups, and whole-workout repeats. Steps and
groups can be reordered. All durations are in seconds. Repeats range from 1-100;
each workout allows up to 50 groups, 50 steps per group, and 10,000 expanded steps.

Start, pause/resume, and restart using the player buttons. Manual steps wait
indefinitely for Continue, Space, or Enter. Space/Enter also starts or pauses
when no interactive control has keyboard focus. Playback is local to each
visitor and does not change the saved timer. The player uses elapsed time
rather than counting ticks; late browser callbacks catch up timed intervals but
never skip a manual rest. Keep the page open and the device awake: browsers may
throttle background tabs or suspend pages during sleep. No audio cues are played.

Validate the timer engine with `node --test tests/fitness-core.test.cjs` and the
library routes with `cargo test --bin wasd5 fitness`.

## Breathing guide

The Health menu links to `/health/box-breathing`, an interactive tool with
a four-second-per-edge box pattern and a 4-7-8 triangle. The triangle's dot
moves continuously: inhale along one edge for 4 seconds, hold along the next
for 7, and exhale along the last for 8, with no extra pauses. Both patterns
can be paused; reduced-motion users start with a stationary guide.
This tool is served by its own route and template, not a SQL Markdown article.

## start by creating an admin user

```bash
cargo run --bin cli -- create-user admin mypassword --role admin
```

the run the website:
```bash
## obtain random rescret:
head -c64 /dev/urandom | base64
## add that to Rocket.toml in production
cargo run --bin wasd5 --release
```


and login in to the admin panel to create pages

## SQL pages

Pages (including the homepage at `landing/landing`) are stored only in SQLite,
using `DATABASE_URL` (default: `wasd5.db`). Migrations run when the site or CLI
starts. Back up the database before migrating an existing installation, then
import the old Markdown files once, attributing them to existing user ID 1:

```bash
cargo run --bin pages -- import --creator-id 1
```

The import is transactional, preserves available filesystem timestamps, and
never overwrites existing SQL pages. Original authors and old revisions cannot
be reconstructed; user 1's username is recorded as creator and initial editor.
The old `pages` directory is retained as a backup and is never read by the site.
Repeated imports skip existing slugs. If an old `landing.md` exists, it is
imported as a separate page; the homepage uses `landing/landing`.

Page views show creation time, creator, last edit, and distinct editors. Actual
content or visibility changes save a complete revision; unchanged saves do not.
Admins can inspect revision snapshots from the edit form or the CLI. Hidden
drafts are only visible to signed-in admins, including by direct URL. The
dashboard lists drafts so they can be found and published. Browser edits use
the signed-in username for attribution.

Markdown articles and homepage content support tables and task lists.
Use `- [ ] Task` for an unchecked box and `- [x] Task` for a checked box.
These boxes are read-only; change their state by editing the Markdown.

## Page management CLI

```bash
cargo run --bin pages -- list
cargo run --bin pages -- edit 1
cargo run --bin pages -- create folder/new-page article.md --hidden
cargo run --bin pages -- hide 1
cargo run --bin pages -- publish 1
cargo run --bin pages -- history 1
```

Set the editor command and attribution name in your environment:

```powershell
$env:EDITOR = 'code --wait'
$env:WASD_PAGE_EDITOR = 'Your name'
```

```bash
export EDITOR='vi'
export WASD_PAGE_EDITOR='Your name'
```

`VISUAL` (preferred) or `EDITOR` may contain arguments (quote executable paths containing spaces).
Use an editor that waits until editing finishes, such as `code --wait`.
`--editor "Your name"` overrides `WASD_PAGE_EDITOR`. When attribution is unset,
the CLI explains how to set it and asks who is editing; empty names are rejected.
Edits use a temporary Markdown file, but SQL remains the source of truth.
Failed saves keep the temporary file and print its location. Concurrent stale
edits are rejected instead of silently overwriting newer revisions.
