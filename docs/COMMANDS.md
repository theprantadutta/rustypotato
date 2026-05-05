# Command Reference

RustyPotato implements a subset of the Redis 7 command surface. Everything
listed below is registered, served over the RESP protocol, and covered by
integration tests. Commands not listed are not implemented — even if real
Redis has them.

The protocol is RESP2 with binary-safe bulk strings. Existing Redis client
libraries (redis-py, ioredis, go-redis, jedis, …) work without
modification.

---

## Connection / server

| Command | Description |
|---|---|
| `PING [message]` | Returns `PONG`, or `message` if provided. |
| `ECHO message` | Returns `message` verbatim. |
| `QUIT` | Replies `+OK` and closes the connection. |
| `INFO [section]` | Server info. Sections: `server`, `clients`, `memory`, `keyspace`. With no argument returns all. |
| `DBSIZE` | Number of keys currently in the store. |
| `TYPE key` | One of `string`, `integer`, `hash`, `none`. |
| `KEYS pattern` | All keys matching a glob pattern (`*`, `?`, `[abc]`, `\` escapes). O(N). |
| `FLUSHDB` | Removes every key. |
| `COMMAND [COUNT \| DOCS]` | Server command introspection used by clients. |
| `BGREWRITEAOF` | Compact the AOF on disk into the smallest replay-equivalent. Async, returns immediately. |

Notes:
- `KEYS` walks the entire keyspace and is intended for ops/debugging, not
  for hot-path use. Use it sparingly.
- `BGREWRITEAOF` is safe to call while writes are in flight; new commands
  hitting during a rewrite are buffered and merged into the new file
  before it replaces the old one.

---

## String / generic key

| Command | Description |
|---|---|
| `SET key value [NX \| XX] [EX s \| PX ms \| EXAT ts \| PXAT tsms \| KEEPTTL] [GET]` | Set a key. See option semantics below. |
| `GET key` | Get a value. Returns nil if missing. |
| `DEL key [key ...]` | Delete one or more keys. Returns the count actually removed. |
| `EXISTS key [key ...]` | Returns the count of keys that exist (counts duplicates). |
| `MGET key [key ...]` | Returns an array of values; nil for missing keys. |
| `MSET key value [key value ...]` | Atomic batch set. Returns `+OK`. |

`SET` options (Redis-compatible):

- `NX` — only set if key doesn't exist. Returns nil if key exists.
- `XX` — only set if key already exists. Returns nil if key missing.
- `EX s` — TTL in seconds.
- `PX ms` — TTL in milliseconds.
- `EXAT ts` — TTL as a Unix timestamp (seconds).
- `PXAT tsms` — TTL as a Unix timestamp (milliseconds).
- `KEEPTTL` — keep the key's existing TTL on overwrite. Without this,
  `SET` clears any prior TTL.
- `GET` — return the previous value (nil if none) instead of `+OK`.

Values are arbitrary bytes — no UTF-8 requirement. `SET` accepts any
binary input and `GET` returns it byte-for-byte.

---

## Atomic counters

| Command | Description |
|---|---|
| `INCR key` | Increment by 1. Errors with `ERR value is not an integer or out of range` on non-integer values. Errors with `ERR increment or decrement would overflow` at `i64::MAX`. |
| `DECR key` | Decrement by 1. Same error semantics. |

If the key doesn't exist, `INCR` / `DECR` create it as `1` / `-1`. The
key is stored as `ValueType::Integer`, so `TYPE` returns `integer` and
ASCII-decimal serialization happens at the protocol boundary.

---

## TTL

| Command | Description |
|---|---|
| `EXPIRE key seconds [NX \| XX \| GT \| LT]` | Set TTL in seconds. Returns 1 if applied, 0 if not. |
| `PEXPIRE key milliseconds [NX \| XX \| GT \| LT]` | Set TTL in milliseconds. |
| `EXPIREAT key unix-time [NX \| XX \| GT \| LT]` | Set TTL as a Unix timestamp (seconds). |
| `PEXPIREAT key unix-time-ms [NX \| XX \| GT \| LT]` | Set TTL as a Unix timestamp (milliseconds). |
| `TTL key` | Remaining TTL in seconds. -1 if no TTL, -2 if key missing. |
| `PTTL key` | Remaining TTL in milliseconds. Same -1/-2 semantics. |

`EXPIRE`-family flags:

- `NX` — only apply if the key has no current TTL.
- `XX` — only apply if the key already has a TTL.
- `GT` — only apply if the new TTL is greater than the current.
- `LT` — only apply if the new TTL is less than the current.

`NX`/`XX` and `GT`/`LT` are mutually exclusive; combining them is a
syntax error.

Expired keys are cleaned up lazily on access and proactively by a
background sweeper.

---

## Hash

| Command | Description |
|---|---|
| `HSET key field value [field value ...]` | Set one or more fields. Returns count of *new* fields added (existing fields are updated but not counted). |
| `HGET key field` | Get a field's value. Nil if missing. |
| `HMGET key field [field ...]` | Array of values; nil entries for missing fields. |
| `HGETALL key` | All field/value pairs as a flat array `[f1, v1, f2, v2, ...]`. Empty array if key missing. |
| `HKEYS key` | Array of field names. |
| `HVALS key` | Array of values. |
| `HLEN key` | Number of fields. |
| `HEXISTS key field` | 1 if the field exists, 0 otherwise. |
| `HDEL key field [field ...]` | Delete one or more fields. Returns count removed. The key itself is removed when the last field is deleted. |

Hash values, like string values, are arbitrary bytes.

---

## Errors

RustyPotato uses Redis-compatible error prefixes so existing client
libraries surface them correctly:

| Prefix | Meaning |
|---|---|
| `ERR` | Generic error (bad arguments, unknown option, integer parse failure). |
| `WRONGTYPE` | Operation against a key holding the wrong kind of value (e.g. `HSET` on a string key). |
| `LOADING` | Server is replaying its AOF; reject the request and retry. |

Unknown commands surface as:

```
ERR unknown command 'foo', with args beginning with: 'arg1', 'arg2'
```

---

## What's not (yet) implemented

The following common Redis surface is **not** implemented in v0.2.0.
Calling them returns `ERR unknown command`. Each is tracked for a future
release:

- Lists: `LPUSH`, `RPUSH`, `LPOP`, `RPOP`, `LRANGE`, `LLEN`
- Sets: `SADD`, `SREM`, `SMEMBERS`, `SCARD`, `SISMEMBER`
- Sorted sets: `ZADD`, `ZRANGE`, `ZREM`, `ZSCORE`
- Pub/sub: `SUBSCRIBE`, `PSUBSCRIBE`, `PUBLISH`, `UNSUBSCRIBE`
- Auth/security: `AUTH`, `ACL`
- Scripting: `EVAL`, `EVALSHA`
- Transactions: `MULTI`, `EXEC`, `DISCARD`, `WATCH`
- Multi-database: `SELECT`, `MOVE` (only `db0` exists)
- Cluster: any `CLUSTER *` command
- Streams: `XADD`, `XREAD`, etc.

If you need any of these for a specific use case, file an issue — the
order they land in is driven by what real users ask for.
