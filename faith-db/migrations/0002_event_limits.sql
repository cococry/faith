BEGIN;

ALTER TABLE device_event_inboxes
  ADD COLUMN pending_count bigint NOT NULL DEFAULT 0,
  ADD COLUMN pending_bytes bigint NOT NULL DEFAULT 0;

UPDATE device_event_inboxes i SET
  pending_count = q.n, pending_bytes = q.bytes
FROM (SELECT auth_id, device_id, count(*) AS n,
             sum(octet_length(event_data) + 16) AS bytes
      FROM device_events GROUP BY auth_id, device_id) q
WHERE i.auth_id = q.auth_id AND i.device_id = q.device_id;

CREATE TABLE event_storage_usage (
  singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
  pending_bytes bigint NOT NULL CHECK (pending_bytes >= 0)
);
INSERT INTO event_storage_usage
SELECT true, COALESCE(sum(pending_bytes), 0) FROM device_event_inboxes;

CREATE FUNCTION enforce_event_storage_limits() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE bytes bigint;
BEGIN
  IF TG_OP = 'INSERT' THEN
    bytes := octet_length(NEW.event_data) + 16;
    IF bytes > 65535 THEN
      RAISE EXCEPTION 'event payload exceeds limit';
    END IF;
    UPDATE event_storage_usage SET pending_bytes = pending_bytes + bytes
      WHERE singleton AND pending_bytes + bytes <= 67108864;
    IF NOT FOUND THEN
      RAISE EXCEPTION 'global pending event byte limit reached';
    END IF;
    UPDATE device_event_inboxes
      SET pending_count = pending_count + 1, pending_bytes = pending_bytes + bytes
      WHERE auth_id = NEW.auth_id AND device_id = NEW.device_id
        AND pending_count < 4096 AND pending_bytes + bytes <= 1048576;
    IF NOT FOUND THEN
      RAISE EXCEPTION 'device pending event limit reached';
    END IF;
    RETURN NEW;
  END IF;
  bytes := octet_length(OLD.event_data) + 16;
  UPDATE event_storage_usage SET pending_bytes = pending_bytes - bytes WHERE singleton;
  UPDATE device_event_inboxes
    SET pending_count = pending_count - 1, pending_bytes = pending_bytes - bytes
    WHERE auth_id = OLD.auth_id AND device_id = OLD.device_id;
  RETURN OLD;
END;
$$;

CREATE TRIGGER event_storage_limits BEFORE INSERT OR DELETE ON device_events
  FOR EACH ROW EXECUTE FUNCTION enforce_event_storage_limits();
CREATE INDEX device_events_expiration_idx ON device_events(created_at);

CREATE FUNCTION expire_device_events() RETURNS TABLE (
  expired_auth bytea, expired_device bytea, expired_until bigint
) LANGUAGE plpgsql AS $$
DECLARE inbox record; cutoff bigint;
BEGIN
  UPDATE event_storage_usage SET pending_bytes = pending_bytes WHERE singleton;
  FOR inbox IN SELECT DISTINCT auth_id, device_id FROM (
    SELECT auth_id, device_id FROM device_events
    WHERE created_at < now() - interval '7 days' ORDER BY created_at LIMIT 256
  ) candidates LIMIT 16
  LOOP
    PERFORM 1 FROM device_event_inboxes
      WHERE auth_id = inbox.auth_id AND device_id = inbox.device_id FOR UPDATE;
    SELECT max(seq_num) INTO cutoff FROM (
      SELECT seq_num FROM device_events
      WHERE auth_id = inbox.auth_id AND device_id = inbox.device_id
        AND seq_num < COALESCE((SELECT min(seq_num) FROM device_events
          WHERE auth_id = inbox.auth_id AND device_id = inbox.device_id
            AND created_at >= now() - interval '7 days'), 9223372036854775807)
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
