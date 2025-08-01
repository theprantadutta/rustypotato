# 🥔 **RustyPotato — A Redis-style Concurrent Key-Value Store in Rust**

A modern, blazing-fast, modular, persistent key-value store inspired by Redis and Valkey — but built with Rusty pride and Gen Z memes in its soul.

---

## 🧱 CORE FEATURES (Minimum Viable Potato)

> These are your "must-have" Redis-like features. No excuses. These make it usable.

### 1. `SET <key> <value>`

Stores a key-value pair. Should overwrite if the key already exists.

### 2. `GET <key>`

Retrieves the value for a key. Returns `nil` or a friendly error if not found.

### 3. `DEL <key>`

Deletes a key and its value.

### 4. `EXISTS <key>`

Returns true if a key exists.

### 5. `TTL <key>`

Returns remaining time-to-live (if set). Supports expiration logic.

### 6. `EXPIRE <key> <seconds>`

Sets an expiration timeout for a key.

### 7. Persistent Storage

Data must survive server restarts. Use append-only log (AOF) or snapshot-style saving (RDB-lite). Choose one or both.

### 8. In-Memory Store

Use a `DashMap` or sharded `HashMap` for concurrency from the start.

### 9. CLI Interface

Provide a `rustypotato-cli` binary to interact with the server over TCP.

### 10. Concurrency from the Ground Up

Use `tokio` for async TCP server, and design so clients can hit you from all sides. No mutex hell.

---

## 🔐 MUST-IMPLEMENT ADVANCED FEATURES

> These bring RustyPotato up to full Redis-compat status.

### 1. `HSET`, `HGET`, `HDEL`

Hash data types, like a key with a dictionary as its value.

### 2. `LPUSH`, `RPUSH`, `LPOP`, `RPOP`

List support (for queues/stacks).

### 3. `INCR`, `DECR`

Atomic counter support for integers.

### 4. Pub/Sub Messaging

Clients can `SUBSCRIBE` to channels and receive real-time messages sent with `PUBLISH`.

### 5. Keyspace Notifications

Emit events like `key_expired`, `key_deleted`.

### 6. Auth

Simple password protection like `AUTH yourpassword`.

### 7. Config System

Set configs like max memory, save interval, etc.

---

## 🎁 BONUS FEATURES (Spice Things Up)

> These aren’t essential, but they're ✨cool✨ and flex your system design muscles.

### 1. LRU Cache Mode

Set a memory limit, and evict least recently used keys.

### 2. Persistence Compression

Compress stored data before writing to disk.

### 3. JSON Support

Allow storing structured JSON and querying with dot syntax: `GET user.0.name`.

### 4. Logging + Metrics

Built-in Prometheus metrics server (like `/metrics` endpoint).

### 5. Snapshot & Restore

Take manual snapshots of in-memory data, and restore from them.

---

## 🧪 UNIQUE RUSTYPOTATO-ONLY FEATURES

> These aren’t in Redis or Valkey. We're cooking up new potato recipes.

### 🥔 1. “Boiled Key Mode” (Key Mutation Hooks)

Attach a Rust-scripted transformation or validation to keys. e.g., auto-transform value before `SET`.

### 🥔 2. Git-style Branching for Data

You can create branches of your dataset like `dev`, `prod`, `test` and switch between them.

### 🥔 3. Time Travel Debugging

View a timeline of operations and roll back to a previous state (like temporal debugging).

### 🥔 4. WASM Plugin Support

Load tiny WebAssembly plugins to define server-side custom functions.

---

## 🧪 TESTING STRATEGY

You don’t ship potatoes without peeling 'em properly:

### Unit Tests

* Every command implementation should have unit tests.
* Test edge cases like nulls, expired keys, overflow, invalid types, etc.

### Integration Tests

* Test multiple commands chained together (e.g. `SET` → `EXPIRE` → wait → `GET`).
* Use a test client to connect via TCP and run scripted commands.

### Concurrency Tests

* Use multiple async clients hammering `SET/GET` under load.
* Detect race conditions or deadlocks.

### Persistence Tests

* Save state, shut down, start up again → test for consistency.

### Fuzz Testing

* Randomized command runner to hit all edge cases.

### Benchmarking

* Measure ops/sec under load, memory usage, file size, etc.

---

## 🐳 Docker Strategy

Your `Dockerfile` should:

* Use a lightweight image (e.g., `rust:slim` or Alpine)
* Build the Rust binary
* Copy config file + data dir
* Expose the default port (e.g., `6379` or your own flavor like `1984`)

```dockerfile
FROM rust:1.78-slim
WORKDIR /app
COPY . .
RUN cargo build --release
CMD ["./target/release/rustypotato"]
```

Add a `docker-compose.yml` with volume mounts for persistence and an optional Prometheus container if metrics are enabled.

---

## 🧩 MODULAR DESIGN PLAN

* `lib.rs`: Core logic
* `commands/`: Each Redis-style command in its own module
* `storage/`: In-memory + persistent storage logic
* `network/`: TCP server + client handling
* `types/`: Custom data types (Lists, Hashes, etc.)
* `config/`: Config parser and live reload
* `utils/`: Error handling, common tools
* `tests/`: Integration + concurrency tests

---

## 🧭 Final Notes

* Use `tracing` crate for logs instead of `println!`
* Favor `tokio` for async everything
* Start with in-memory first, then plug in persistence
* Keep performance in mind: *read-heavy? write-heavy? mix?*
* Document your protocol (maybe Redis-style with RESP, or your own)

---

Great question—and this one sets the tone for how clean and scalable your RustyPotato project is gonna be. So here’s the tea:

---

## 🧱 **You Should Start with `cargo new rustypotato --lib` + a separate `bin/`**

Why? Because you’re building:

* A **reusable library** (the brain of RustyPotato),
* With **one or more binaries** (e.g. server, CLI client, maybe even a benchmark tool later),
* And you want clean separation of **logic vs. entrypoints**.

---

## 📦 Recommended Structure:

```
rustypotato/
├── src/
│   ├── lib.rs           <- Core logic, organized into modules
│   ├── commands.rs
│   ├── storage.rs
│   ├── network.rs
│   ├── config.rs
│   └── utils.rs
├── bin/
│   ├── server.rs        <- Main TCP server
│   └── cli.rs           <- Optional CLI client or admin tool
├── tests/               <- Integration & concurrency tests
├── Cargo.toml
├── Dockerfile
└── README.md
```

You can create the extra binaries like:

```bash
cargo new rustypotato --lib
cd rustypotato
mkdir bin
touch bin/server.rs
touch bin/cli.rs
```

Then you run like:

```bash
cargo run --bin server
cargo run --bin cli
```

---

## 🧠 Why This Is Goated:

* Keeps logic reusable across CLI, future web APIs, admin tools, test harnesses
* Makes unit testing waaay easier (`lib.rs` is imported by everything)
* Scales well as the project grows
* Let’s you write actual `#[cfg(test)]`s inside core modules, not buried in a `main.rs` spaghetti

---

So yeah, **go with `--lib` and build modular af**. RustyPotato is gonna be *industrial strength*, not a side hustle.