# lib

the shared core library for all Black Mesa services. provides Discord API models, database layer (PostgreSQL via sqlx), Redis cache, mesastream client, and telemetry setup. used by `black-mesa`, `api`, and `mesastream`.

## what's in here

### discord models
- gateway events (MessageCreate, GuildMemberAdd, VoiceStateUpdate, etc.)
- REST API models (Guild, Channel, Role, Member, User)
- REST client for bot API calls
- utility types (snowflake IDs, permissions, etc.)

### database layer
- `Database` - PostgreSQL connection pool via sqlx
- migrations embedded in binary (auto-run on startup)
- models: `GuildConfig`, `Infraction`, `LogConfig`, `AutomodSettings`
- query builders for infractions, guild configs, logging

### cache
- `Cache<T>` - generic caching layer
- `RedisCache` - Redis-backed implementation
- TTL support, key prefixing, bulk operations
- used for caching guild configs, Discord API responses, session state

### clients
- `MesastreamClient` - HTTP client for mesastream API
- `MesastreamWsClient` - WebSocket client for mesastream events
- `DiscordRestClient` - Discord REST API wrapper

### permissions
- custom permission system with bitflags
- permission resolution via config groups, roles, users
- Discord permission inheritance

### logging
- `LogEventType` enum for all supported log events
- template variable rendering system
- embed builder for Discord messages

## migrations

migrations live in `migrations/` and are embedded at compile time. they run automatically on `Database::migrate()`. add new migrations with:

```bash
sqlx migrate add <name>
```

then edit the generated `.sql` file.

## usage

its pretty purpose made for black mesa so i don't recommend using it for anything else but if you want to, go ahead. the API is not super stable since it's primarily intended for internal use, but if you find something useful or want to contribute, open a PR or issue.

## telemetry

all services use OpenTelemetry for distributed tracing. the `telemetry::init()` function sets up the OTLP exporter with propagation context. configure via env vars:

- `OTLP_ENDPOINT` - OTLP endpoint URL
- `OTLP_AUTH` - optional auth header
- `OTLP_ORGANIZATION` - optional org/tenant ID

## db schema

key tables:

- `guilds` - guild configurations (prefix, mute_role, modules, automod, permission_groups, etc.)
- `infractions` - moderation actions (warns, mutes, kicks, bans) with expiry and active status
- `log_configs` - per-event logging configuration (channel, template, embed settings)

schema is managed via sqlx migrations. see `migrations/*.sql` for DDL.
