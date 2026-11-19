BEGIN;

CREATE TABLE conversations (
  conversation_id bytea PRIMARY KEY CHECK (octet_length(conversation_id) = 16),
  owner_id bytea NOT NULL REFERENCES users(auth_id) ON DELETE CASCADE
);

CREATE TABLE conversation_requests (
  conversation_id bytea NOT NULL REFERENCES conversations(conversation_id) ON DELETE CASCADE,
  recipient_id bytea NOT NULL REFERENCES users(auth_id) ON DELETE CASCADE,
  accepted boolean,
  PRIMARY KEY (conversation_id, recipient_id)
);

-- Conversation requests and responses stay in the ordered inbox until ACK.
-- Expiry can only remove a contiguous prefix before the first retained event.
CREATE INDEX device_events_conversation_pending_idx
  ON device_events(auth_id, device_id, seq_num) WHERE event_type IN (2, 3);

CREATE OR REPLACE FUNCTION expire_device_events() RETURNS TABLE (
  expired_auth bytea, expired_device bytea, expired_until bigint
) LANGUAGE plpgsql AS $$
DECLARE inbox record; cutoff bigint;
BEGIN
  UPDATE event_storage_usage SET pending_bytes = pending_bytes WHERE singleton;
  FOR inbox IN SELECT DISTINCT auth_id, device_id FROM (
    SELECT e.auth_id, e.device_id FROM device_events e
    WHERE e.created_at < now() - interval '7 days'
      AND NOT EXISTS (
        SELECT 1 FROM device_events protected
        WHERE protected.auth_id = e.auth_id AND protected.device_id = e.device_id
          AND protected.seq_num <= e.seq_num AND protected.event_type IN (2, 3)
      )
    ORDER BY e.created_at LIMIT 256
  ) candidates LIMIT 16
  LOOP
    PERFORM 1 FROM device_event_inboxes
      WHERE auth_id = inbox.auth_id AND device_id = inbox.device_id FOR UPDATE;
    SELECT max(seq_num) INTO cutoff FROM (
      SELECT seq_num FROM device_events
      WHERE auth_id = inbox.auth_id AND device_id = inbox.device_id
        AND seq_num < COALESCE((SELECT min(seq_num) FROM device_events
          WHERE auth_id = inbox.auth_id AND device_id = inbox.device_id
            AND (created_at >= now() - interval '7 days' OR event_type IN (2, 3))), 9223372036854775807)
      ORDER BY seq_num LIMIT 256
    ) q;
    IF cutoff IS NOT NULL THEN
      DELETE FROM device_events WHERE auth_id = inbox.auth_id
        AND device_id = inbox.device_id AND seq_num <= cutoff;
      UPDATE device_event_inboxes SET last_acked_seq = GREATEST(last_acked_seq, cutoff)
        WHERE auth_id = inbox.auth_id AND device_id = inbox.device_id;
      expired_auth := inbox.auth_id;
      expired_device := inbox.device_id;
      expired_until := cutoff;
      RETURN NEXT;
    END IF;
  END LOOP;
END;
$$;

COMMIT;
