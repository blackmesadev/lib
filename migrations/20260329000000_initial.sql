-- Initial schema: configs + infractions

CREATE TABLE IF NOT EXISTS configs (
    id                       BIGINT PRIMARY KEY,  -- Discord Guild ID
    prefix                   TEXT NOT NULL DEFAULT '!',
    mute_role                BIGINT,
    default_warn_duration    BIGINT,
    log_channel              BIGINT,
    prefer_embeds            BOOLEAN NOT NULL DEFAULT false,
    inherit_discord_perms    BOOLEAN NOT NULL DEFAULT true,
    alert_on_infraction      BOOLEAN NOT NULL DEFAULT true,
    send_permission_denied   BOOLEAN NOT NULL DEFAULT true,
    permission_groups        JSONB,
    automod                  JSONB,
    command_aliases          JSONB
);

CREATE TABLE IF NOT EXISTS infractions (
    id               UUID PRIMARY KEY,
    guild_id         BIGINT NOT NULL,
    user_id          BIGINT NOT NULL,
    moderator_id     BIGINT NOT NULL,
    infraction_type  TEXT NOT NULL,
    reason           TEXT,
    last_edited      BIGINT,
    expires_at       BIGINT,
    mute_role_id     BIGINT,
    automod_offense  JSONB,
    active           BOOLEAN NOT NULL DEFAULT true
);

CREATE INDEX IF NOT EXISTS idx_infractions_guild ON infractions (guild_id);

CREATE INDEX IF NOT EXISTS idx_infractions_active ON infractions (guild_id, user_id, active)
    WHERE active = true;

CREATE INDEX IF NOT EXISTS idx_infractions_expires ON infractions (expires_at)
    WHERE active = true AND expires_at IS NOT NULL;
