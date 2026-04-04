-- Log configuration per guild per event
-- Allows guild owners to configure how each event is logged:
--   embed vs text, custom content with placeholders, target channel override
CREATE TABLE IF NOT EXISTS log_configs (
    id           BIGSERIAL PRIMARY KEY,
    guild_id     BIGINT NOT NULL REFERENCES configs(id) ON DELETE CASCADE,
    event        TEXT NOT NULL,
    enabled      BOOLEAN NOT NULL DEFAULT true,
    channel_id   BIGINT,
    embed        BOOLEAN NOT NULL DEFAULT true,
    -- For text mode: template with placeholders e.g. "User {{user_id}} was kicked by {{moderator_id}}"
    -- For embed mode: JSON object describing embed layout
    text_content TEXT,
    -- Embed customization (only used when embed = true)
    embed_title  TEXT,
    embed_body   TEXT,
    embed_color  INTEGER,
    embed_footer TEXT,
    UNIQUE (guild_id, event)
);

-- Add logging_enabled flag to main config
ALTER TABLE configs ADD COLUMN IF NOT EXISTS logging_enabled BOOLEAN NOT NULL DEFAULT false;

CREATE INDEX IF NOT EXISTS idx_log_configs_guild ON log_configs (guild_id);
CREATE INDEX IF NOT EXISTS idx_log_configs_guild_event ON log_configs (guild_id, event) WHERE enabled = true;
