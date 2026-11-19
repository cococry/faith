BEGIN;

ALTER TABLE users ADD COLUMN username text COLLATE "C"
  CHECK (username ~ '^[A-Za-z1-9_]{1,20}$');

CREATE UNIQUE INDEX users_username_unique ON users (lower(username))
  WHERE username IS NOT NULL;

COMMIT;
