# wasd5

A Rocket.rs based webapp for my personal site.

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

## Breathing guide

The Health menu links to `/tools/box-breathing`, an interactive tool with
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
