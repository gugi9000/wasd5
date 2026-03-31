CREATE TABLE calendar_allowed_ips (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ip_address TEXT NOT NULL UNIQUE,
  created_at INTEGER NOT NULL
);
