CREATE TABLE pages (
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    slug TEXT NOT NULL UNIQUE,
    content TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    creator TEXT NOT NULL CHECK (length(trim(creator)) > 0),
    hidden BOOLEAN NOT NULL DEFAULT 0
);

CREATE TABLE page_revisions (
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    page_id INTEGER NOT NULL REFERENCES pages(id),
    content TEXT NOT NULL,
    hidden BOOLEAN NOT NULL,
    editor TEXT NOT NULL CHECK (length(trim(editor)) > 0),
    edited_at BIGINT NOT NULL
);
CREATE INDEX page_revisions_page_id ON page_revisions(page_id, id);
