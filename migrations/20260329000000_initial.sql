-- Types
CREATE TYPE infraction_type AS ENUM ('warn', 'mute', 'kick', 'ban');

-- Guild configuration
-- command_aliases: {alias: command}
CREATE TABLE IF NOT EXISTS configs (
    id                       BIGINT PRIMARY KEY,
    prefix                   TEXT NOT NULL DEFAULT '!',
    mute_role                BIGINT,
    default_warn_duration    BIGINT,
    log_channel              BIGINT,
    prefer_embeds            BOOLEAN NOT NULL DEFAULT false,
    inherit_discord_perms    BOOLEAN NOT NULL DEFAULT true,
    alert_on_infraction      BOOLEAN NOT NULL DEFAULT true,
    send_permission_denied   BOOLEAN NOT NULL DEFAULT true,
    command_aliases          JSONB,
    automod                  JSONB,
    -- Module enabled flags
    automod_enabled          BOOLEAN NOT NULL DEFAULT false,
    music_enabled            BOOLEAN NOT NULL DEFAULT false,
    moderation_enabled       BOOLEAN NOT NULL DEFAULT false
);

-- Permission groups
CREATE TABLE IF NOT EXISTS permissions (
    id           BIGSERIAL PRIMARY KEY,
    guild_id     BIGINT NOT NULL REFERENCES configs(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    permissions  BIGINT NOT NULL DEFAULT 0,
    roles        BIGINT[] NOT NULL DEFAULT '{}',
    users        BIGINT[] NOT NULL DEFAULT '{}'
);

-- Infractions (warnings, mutes, kicks, bans)
-- automod_offense: {type, message, count, interval, offending_filter}
CREATE TABLE IF NOT EXISTS infractions (
    id               UUID PRIMARY KEY,
    guild_id         BIGINT NOT NULL,
    user_id          BIGINT NOT NULL,
    moderator_id     BIGINT NOT NULL,
    infraction_type  infraction_type NOT NULL,
    reason           TEXT,
    last_edited      BIGINT,
    expires_at       BIGINT,
    mute_role_id     BIGINT,
    automod_offense  JSONB,
    active           BOOLEAN NOT NULL DEFAULT true
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_permissions_guild ON permissions (guild_id);
CREATE INDEX IF NOT EXISTS idx_permissions_bitwise ON permissions (guild_id, permissions);
CREATE INDEX IF NOT EXISTS idx_permissions_users ON permissions USING GIN (users);

CREATE INDEX IF NOT EXISTS idx_infractions_guild ON infractions (guild_id);

CREATE INDEX IF NOT EXISTS idx_infractions_type ON infractions (guild_id, infraction_type, active)
    WHERE active = true;

CREATE INDEX IF NOT EXISTS idx_infractions_user ON infractions (guild_id, user_id, active);

CREATE INDEX IF NOT EXISTS idx_infractions_expires ON infractions (expires_at)
    WHERE active = true AND expires_at IS NOT NULL;
