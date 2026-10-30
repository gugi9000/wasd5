CREATE TABLE comic_albums (
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    owner_id INTEGER NOT NULL REFERENCES users(id),
    title TEXT NOT NULL CHECK (length(trim(title)) BETWEEN 1 AND 200),
    subtext TEXT NOT NULL DEFAULT '',
    hidden BOOLEAN NOT NULL DEFAULT 1,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE INDEX comic_albums_owner ON comic_albums(owner_id);

CREATE TABLE comic_strips (
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    album_id INTEGER NOT NULL REFERENCES comic_albums(id),
    title TEXT NOT NULL CHECK (length(trim(title)) BETWEEN 1 AND 200),
    subtext TEXT NOT NULL DEFAULT '',
    image_filename TEXT NOT NULL UNIQUE,
    hidden BOOLEAN NOT NULL DEFAULT 1,
    display_order INTEGER NOT NULL CHECK (display_order >= 0),
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE INDEX comic_strips_album_order ON comic_strips(album_id, display_order, id);
