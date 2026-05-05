# Client libraries

RustyPotato speaks RESP2 with binary-safe bulk strings, so any existing
Redis client library works without modification. You don't need a
"RustyPotato SDK" — point your Redis client at the rustypotato port and
use the [supported commands](COMMANDS.md).

The examples below are minimal connect-and-do-stuff recipes for the
most common languages. They assume a server running on the default
`127.0.0.1:6379`.

---

## Python — redis-py

```python
import redis

r = redis.Redis(host="127.0.0.1", port=6379, decode_responses=True)

r.set("greeting", "hello world", ex=60)
print(r.get("greeting"))         # "hello world"

r.hset("user:1", mapping={"name": "ada", "role": "admin"})
print(r.hgetall("user:1"))       # {"name": "ada", "role": "admin"}

r.incr("counter")
r.incr("counter")
print(r.get("counter"))          # "2"
```

For binary values (which RustyPotato handles natively), drop the
`decode_responses=True` flag and you'll get `bytes` back from `GET`.

---

## Node.js — ioredis

```javascript
const Redis = require("ioredis");

const r = new Redis({ host: "127.0.0.1", port: 6379 });

await r.set("greeting", "hello world", "EX", 60);
console.log(await r.get("greeting"));         // "hello world"

await r.hset("user:1", { name: "ada", role: "admin" });
console.log(await r.hgetall("user:1"));       // { name: "ada", role: "admin" }

await r.incr("counter");
await r.incr("counter");
console.log(await r.get("counter"));          // "2"

await r.quit();
```

---

## Go — go-redis

```go
package main

import (
    "context"
    "fmt"
    "time"

    "github.com/redis/go-redis/v9"
)

func main() {
    ctx := context.Background()
    rdb := redis.NewClient(&redis.Options{Addr: "127.0.0.1:6379"})
    defer rdb.Close()

    rdb.Set(ctx, "greeting", "hello world", time.Minute)
    val, _ := rdb.Get(ctx, "greeting").Result()
    fmt.Println(val) // "hello world"

    rdb.HSet(ctx, "user:1", "name", "ada", "role", "admin")
    fmt.Println(rdb.HGetAll(ctx, "user:1").Val())

    rdb.Incr(ctx, "counter")
    rdb.Incr(ctx, "counter")
    fmt.Println(rdb.Get(ctx, "counter").Val()) // "2"
}
```

---

## Rust — `redis` crate

```rust
use redis::Commands;

fn main() -> redis::RedisResult<()> {
    let client = redis::Client::open("redis://127.0.0.1:6379/")?;
    let mut con = client.get_connection()?;

    let _: () = con.set_ex("greeting", "hello world", 60)?;
    let g: String = con.get("greeting")?;
    println!("{g}");

    let _: () = con.hset_multiple("user:1", &[("name", "ada"), ("role", "admin")])?;
    let user: std::collections::HashMap<String, String> = con.hgetall("user:1")?;
    println!("{user:?}");

    let _: i64 = con.incr("counter", 1)?;
    let _: i64 = con.incr("counter", 1)?;
    let c: String = con.get("counter")?;
    println!("{c}"); // "2"

    Ok(())
}
```

---

## CLI — redis-cli

The Redis distribution's `redis-cli` works as a drop-in. Useful for
scripting and ad-hoc poking:

```bash
redis-cli -h 127.0.0.1 -p 6379 ping            # PONG
redis-cli -h 127.0.0.1 -p 6379 set foo bar EX 60
redis-cli -h 127.0.0.1 -p 6379 get foo
redis-cli -h 127.0.0.1 -p 6379 hset h a 1 b 2 c 3
redis-cli -h 127.0.0.1 -p 6379 hgetall h
redis-cli -h 127.0.0.1 -p 6379 keys '*'
redis-cli -h 127.0.0.1 -p 6379 info server
```

`redis-benchmark` also works against rustypotato-server, though for an
honest comparison see [BENCHMARKING.md](BENCHMARKING.md).

---

## Known compatibility gaps

The following behaviors differ from real Redis and may surprise client
libraries that rely on them:

- **Single database only.** `SELECT 1` returns an unknown-command
  error. There is no `db1`, `db2`, etc.
- **No transactions.** `MULTI`/`EXEC`/`DISCARD`/`WATCH` are not
  implemented; clients that use pipelining-with-transactions will get
  unknown-command errors on the `MULTI`.
- **No pub/sub.** Clients calling `SUBSCRIBE` see an unknown-command
  error rather than the long-poll behavior.
- **No scripting.** `EVAL`, `EVALSHA`, `FUNCTION` are not implemented.
- **No `CLIENT *` subcommands.** Things like `CLIENT GETNAME`,
  `CLIENT KILL` are not implemented; client connection tracking is
  internal-only.
- **No `CONFIG GET / CONFIG SET`.** Configuration is loaded at startup
  from file/env/CLI and is immutable for the lifetime of the process.

If any of these matter for your use case, file an issue with the client
library and the call site — implementation order is driven by what
actual users hit.
