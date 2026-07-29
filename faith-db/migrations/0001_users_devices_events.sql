BEGIN;

CREATE TABLE users (
  auth_id bytea PRIMARY KEY,

  created_at timestamptz NOT NULL DEFAULT now(),

  CONSTRAINT users_auth_id_size
    CHECK (octet_length(auth_id) = 16)
);

CREATE TABLE devices (
  auth_id bytea NOT NULL,
  device_id bytea NOT NULL,
  public_key bytea NOT NULL, 

  registered_at timestamptz NOT NULL DEFAULT now(),
  revoke_at timestamptz,

  PRIMARY KEY (auth_id, device_id),

  CONSTRAINT devices_users_fk
    FOREIGN KEY (auth_id)
    REFERENCES users(auth_id)
    ON DELETE CASCADE, 

  CONSTRAINT devices_auth_id_size
    CHECK (octet_length(auth_id) = 16),

  CONSTRAINT devices_device_id_size
    CHECK (octet_length(device_id) = 16),

  CONSTRAINT devices_public_key_size
    CHECK (octet_length(public_key) = 32)
);

CREATE TABLE device_events (
  event_id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,

  auth_id bytea NOT NULL,
  device_id bytea NOT NULL,

  event_type integer NOT NULL,
  event_data bytea NOT NULL,


  created_at timestamptz NOT NULL DEFAULT now(),
  acknowledged_at timestamptz,

  CONSTRAINT device_events_device_fk
    FOREIGN KEY (auth_id, device_id)
    REFERENCES devices(auth_id, device_id)
    ON DELETE CASCADE,
    
  CONSTRAINT device_events_auth_id_size
    CHECK (octet_length(auth_id) = 16),

  CONSTRAINT device_events_device_id_size
    CHECK (octet_length(device_id) = 16),

  CONSTRAINT device_events_type_range
    CHECK (event_type >= 0)
);

CREATE INDEX device_events_pending_idx
    ON device_events (auth_id, device_id, event_id)
    WHERE acknowledged_at IS NULL;

COMMIT;
