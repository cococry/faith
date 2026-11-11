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

CREATE TABLE device_event_inboxes (
  auth_id bytea NOT NULL,
  device_id bytea NOT NULL,

  next_seq bigint NOT NULL DEFAULT 0,

  last_acked_seq bigint,

  PRIMARY KEY (auth_id, device_id),

  CONSTRAINT device_inboxes_device_fk 
    FOREIGN KEY (auth_id, device_id)
    REFERENCES devices(auth_id, device_id)
    ON DELETE CASCADE,

  CONSTRAINT device_inboxes_auth_id_size
    CHECK (octet_length(auth_id) = 16),

  CONSTRAINT device_inboxes_device_id_size
    CHECK (octet_length(device_id) = 16),

  CONSTRAINT device_inboxes_next_seq_range
    CHECK (next_seq >= 0),

  CONSTRAINT device_inboxes_last_acked_range
    CHECK (last_acked_seq IS NULL OR last_acked_seq >= 0),

  CONSTRAINT device_inboxes_ack_before_next
    CHECK (last_acked_seq IS NULL OR last_acked_seq < next_seq)
);

CREATE TABLE device_events (

  auth_id bytea NOT NULL,
  device_id bytea NOT NULL,
  seq_num bigint NOT NULL,

  event_type integer NOT NULL,
  event_data bytea NOT NULL,
  
  created_at timestamptz NOT NULL DEFAULT now(),
  
  PRIMARY KEY (auth_id, device_id, seq_num),

  CONSTRAINT device_events_inbox_fk
    FOREIGN KEY (auth_id, device_id)
    REFERENCES device_event_inboxes(auth_id, device_id)
    ON DELETE CASCADE,

  CONSTRAINT device_events_auth_id_size
    CHECK (octet_length(auth_id) = 16),

  CONSTRAINT device_events_device_id_size
    CHECK (octet_length(device_id) = 16),

  CONSTRAINT device_events_seq_range
    CHECK (seq_num >= 0)
);

COMMIT;
