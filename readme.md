# 🚦 Distributed Rate Limiting Service

A scalable, production-ready **rate limiting API** built with **Rust** (core engine) and **Elixir Phoenix** (API & dashboard). Supports multiple algorithms, multi-tenant configs, and billing integration.

---

## 📌 Features

- Token Bucket & Sliding Window algorithms
- Redis-based distributed sync
- REST API + API key auth
- Multi-tenant support
- Phoenix LiveView dashboard
- Stripe subscription billing
- Dockerized & ready to deploy

---

## 🧱 Tech Stack

| Layer            | Tech               |
|------------------|--------------------|
| Core Engine      | Rust               |
| API + Web UI     | Elixir (Phoenix)   |
| Coordination     | Redis              |
| Billing          | Stripe             |
| Deployment       | Docker, Fly.io     |

---

## ⚙️ Setup

### Prerequisites
- Rust (`cargo`)
- Elixir & Phoenix
- Redis
- Docker (optional for full setup)

### 1. Clone the repo

```bash
git clone https://github.com/yourusername/rate-limiter.git
cd rate-limiter




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
├── core/       # Rust rate limiter
├── api/        # Phoenix API & dashboard
├── sdk/        # Client SDKs (optional)
├── billing/    # Stripe configs
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
