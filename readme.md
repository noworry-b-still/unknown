# 🚦 Distributed Rate Limiting Service

A scalable, production-ready **rate limiting API** being built with **Rust** (core engine) and **Elixir/Phoenix** (API & dashboard). Supports multiple algorithms, multi-tenant configs, and billing integration.

---

## 🧱 Tech Stack

| Layer            | Tech               | Status                           |
|------------------|--------------------|----------------------------------|
| Core Engine      | Rust               | ✅ Phase 1 complete, Phase 2 WIP |
| API + Web UI     | Elixir (Phoenix)   | ⏳ Pending                        |
| Coordination     | Redis              | ⏳ Pending                        |
| Billing          | Stripe             | ⏳ Pending                        |
| Deployment       | Docker, Fly.io     | ⏳ Pending                        |


---

## 📌 Features

- Token Bucket, Sliding Window, Fixed Window algorithms
- Redis-based distributed sync
- Circuit breaker, health checks, and fallback mechanisms
- REST API with API key authentication
- Multi-tenant support
- Phoenix LiveView dashboard
- Stripe subscription billing
- Dockerized and deploy-ready


---

## 🧩 Architecture Phases

### ✅ Phase 1: Core Engine
- Rate limiting algorithms (Token Bucket, Sliding, Fixed)
- In-memory + Redis backends
- Resilience: Circuit breaker, health checks, fallback

### 🚧 Phase 2: Distributed Architecture
- Service discovery
- Peer-to-peer communication
- Distributed Consensus Implementation
- Rate Limit Distribution Strategy 

---

## ⚙️ Setup

### Prerequisites
- Rust (`cargo`)
- Elixir & Phoenix
- Redis
- Docker (for full setup)



### 1. Clone the repo

```bash
git clone https://github.com/yourusername/rate-limiter.git
cd rate-limiter
```



### Start Redis (Optional)
```bash
docker-compose up -d redis
```

### 2. Run Rust Core (Rate Limiter)
```bash
cd core
cargo run
```

### 3. Run Phoenix API Server
```bash
cd api
mix deps.get
mix ecto.setup
mix phx.server
```

> Visit: http://localhost:4000

---

## 📡 Example API Usage

### Create Rate Limit
```bash
curl -X POST http://localhost:4000/api/limits   -H "Authorization: Bearer <api-key>"   -H "Content-Type: application/json"   -d '{
        "key": "user123",
        "limit": 1000,
        "interval": "1m",
        "algorithm": "token_bucket"
      }'
```

### Check Rate Limit
```bash
curl -X GET http://localhost:4000/api/check?key=user123   -H "Authorization: Bearer <api-key>"
```

---

## 🧪 Test Commands

```bash
# Rust tests
cd core
cargo test

# Elixir tests
cd api
mix test
```

---

## 📊 Dashboard

- Auth via email/password
- LiveView dashboard at `/dashboard`
- View usage, limits, and billing info

---

## 💳 Billing

Stripe is integrated (test keys). Plans:

- **Free** — 10k requests/day
- **Pro** — 1M/day
- **Enterprise** — Custom

---

## 📦 Project Structure

```
rate-limiter/
├── src/               # Rust workspace root
│   ├── core/          # Rate limiter engine (algorithms, storage, resilience)
│   ├── sdk/           # Optional Rust client SDKs
│   └── billing/       # Stripe billing logic (if applicable in Rust)
├── api/               # Phoenix API & dashboard
├── docker-compose.yml
└── README.md
```

---

## 🚀 Deployment

Use Docker:

```bash
docker-compose up --build
```

Or deploy `api/` and `core/` separately (Fly.io, Render, etc.)

---

## 📖 License

MIT

---

## ✨ Built with

- Rust 🦀
- Elixir ⚡
- Redis
- Stripe
