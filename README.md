# wasd5

A Rocket.rs based webapp for my personal site.

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

`EDITOR` may contain arguments (quote executable paths containing spaces).
Use an editor that waits until editing finishes, such as `code --wait`.
`--editor "Your name"` overrides `WASD_PAGE_EDITOR`. When attribution is unset,
the CLI explains how to set it and asks who is editing; empty names are rejected.
Edits use a temporary Markdown file, but SQL remains the source of truth.
Failed saves keep the temporary file and print its location. Concurrent stale
edits are rejected instead of silently overwriting newer revisions.
