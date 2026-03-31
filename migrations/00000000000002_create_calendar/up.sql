CREATE TABLE calendar_persons (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL,
  display_order INTEGER NOT NULL DEFAULT 0
);

INSERT INTO calendar_persons (name, display_order) VALUES ('Randi', 0);
INSERT INTO calendar_persons (name, display_order) VALUES ('Bjarke', 1);
INSERT INTO calendar_persons (name, display_order) VALUES ('Fælles', 2);

CREATE TABLE calendar_appointments (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  person_id INTEGER NOT NULL REFERENCES calendar_persons(id),
  title TEXT NOT NULL,
  date TEXT NOT NULL,
  start_time TEXT,
  end_time TEXT,
  created_at INTEGER NOT NULL
);
