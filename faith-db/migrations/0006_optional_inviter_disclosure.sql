BEGIN;

ALTER TABLE conversations
  DROP CONSTRAINT conversations_check,
  ADD CONSTRAINT conversations_required_username_check
    CHECK (NOT force_username_disclosure OR disclosed_username IS NOT NULL);

COMMIT;
