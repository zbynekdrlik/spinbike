# SpinBike PWA — Design Specification

**Date:** 2026-04-09
**Status:** Approved
**Replaces:** Legacy VB6 + MS Access app ("Spinning" by Marcel Markulik)

## Overview

A modern PWA replacing the legacy spin bike reservation and prepaid card management system for Squash Centrum Smizany. Cloud-hosted on Hetzner VPS, accessible to both staff and customers.

## Architecture

**Monolith — single binary**, matching established patterns across all `~/devel/` Rust projects.

```
spinbike/
├── Cargo.toml              # Workspace root (edition 2024)
├── crates/
│   ├── spinbike-core/      # Shared types & domain logic (WASM-safe)
│   └── spinbike-server/    # Axum 0.8 + SQLite (sqlx) + auth + REST + WebSocket
├── spinbike-ui/            # Leptos 0.7 CSR + Trunk (excluded from workspace)
├── VERSION
└── scripts/
    └── sync-version.sh
```

### Crate Responsibilities

**spinbike-core** (targets: native + wasm32)
- Domain models: User, Card, Booking, ClassTemplate, Transaction, Service, Instructor
- Shared enums: Role (Admin/Staff/Customer), Action (Credit/Debit/Activation/Storno)
- Validation logic
- No tokio, no filesystem, no platform-specific deps

**spinbike-server** (targets: native only)
- Axum 0.8 HTTP server with REST API
- WebSocket for live booking updates
- JWT authentication (access + refresh tokens)
- OAuth2 (Google, Facebook) via authorization code flow
- SQLite database via sqlx 0.8
- rust-embed to serve the Trunk-built UI dist/
- Data migration CLI (one-time Access DB import)

**spinbike-ui** (targets: wasm32 only, excluded from workspace)
- Leptos 0.7 CSR, built by Trunk
- Role-based routing (customer views vs staff/admin views)
- PWA manifest + service worker for install-to-homescreen
- gloo-net for HTTP + WebSocket communication

### Tech Stack (consistent with existing projects)

| Layer | Choice |
|-------|--------|
| Backend framework | Axum 0.8 |
| Frontend framework | Leptos 0.7 CSR |
| WASM build tool | Trunk |
| Database | SQLite via sqlx 0.8 |
| Asset embedding | rust-embed 8 |
| Serialization | serde + serde_json |
| Error handling | thiserror 2 + anyhow 1 |
| Logging | tracing + tracing-subscriber |
| HTTP middleware | tower-http 0.6 (cors, trace, fs) |
| Async runtime | tokio 1 (full) |

## Data Model

### users
| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER PK | Auto-increment |
| email | TEXT UNIQUE | Login identifier |
| password_hash | TEXT NULL | NULL for OAuth-only users |
| name | TEXT | Display name |
| phone | TEXT NULL | Optional |
| role | TEXT | "admin", "staff", "customer" |
| oauth_provider | TEXT NULL | "google", "facebook", or NULL |
| oauth_id | TEXT NULL | Provider's user ID |
| created_at | TEXT | ISO 8601 |

### cards
| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER PK | Auto-increment |
| barcode | TEXT UNIQUE | 8-char, format "7070xxxx" |
| user_id | INTEGER NULL | FK to users — NULL if not linked to digital account |
| blocked | INTEGER | 0/1 |
| credit | REAL | EUR balance |
| allow_debit | INTEGER | 0/1 — can go negative (admin-only to set) |
| created_at | TEXT | ISO 8601 |

### services
| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER PK | Auto-increment |
| name | TEXT | "Spinning", "Fitness" |
| default_price | REAL | EUR |
| active | INTEGER | 0/1 |

### transactions
| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER PK | Auto-increment |
| user_id | INTEGER NULL | FK to users — customer |
| card_id | INTEGER NULL | FK to cards — if paid via card |
| staff_id | INTEGER NULL | FK to users — who processed |
| service_id | INTEGER NULL | FK to services |
| amount | REAL | EUR (positive = credit added, negative = debit) |
| action | TEXT | "credit", "debit", "activation", "storno" |
| created_at | TEXT | ISO 8601 |

### instructors
| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER PK | Auto-increment |
| name | TEXT | Display name |
| active | INTEGER | 0/1 |

### class_templates
| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER PK | Auto-increment |
| weekday | INTEGER | 0=Monday, 6=Sunday |
| start_time | TEXT | "17:00" |
| duration_minutes | INTEGER | e.g., 60 |
| instructor_id | INTEGER | FK to instructors |
| capacity | INTEGER | Number of bikes available |
| active | INTEGER | 0/1 — soft delete |

Recurring weekly schedule. A template "Monday 17:00, Judita, capacity 10" generates a class every Monday automatically. No need to create individual class instances.

### class_cancellations
| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER PK | Auto-increment |
| template_id | INTEGER | FK to class_templates |
| date | TEXT | ISO 8601 date of the cancelled occurrence |
| reason | TEXT NULL | Optional |
| cancelled_by | INTEGER | FK to users (staff/admin) |
| created_at | TEXT | ISO 8601 |

Cancel a specific occurrence without breaking the recurring pattern.

### bookings
| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER PK | Auto-increment |
| template_id | INTEGER | FK to class_templates |
| date | TEXT | ISO 8601 date of the class occurrence |
| user_id | INTEGER | FK to users — who's attending |
| created_by | INTEGER | FK to users — self or staff (walk-in) |
| created_at | TEXT | ISO 8601 |
| cancelled_at | TEXT NULL | NULL = active, set = cancelled |

A spot in a class. No bike number — just headcount against capacity. Unique constraint on (template_id, date, user_id) where cancelled_at IS NULL.

### settings
| Column | Type | Notes |
|--------|------|-------|
| key | TEXT PK | e.g., "bike_count", "center_name" |
| value | TEXT | Stored as text, parsed by application |

## Authentication & Authorization

### Auth Flow
- **Customer:** Email+password OR Google/Facebook OAuth → JWT (access token in memory, refresh token in httpOnly cookie)
- **Staff/Admin:** Email+password → JWT
- **Card linking:** Logged-in customer enters barcode → cards.user_id set to their user ID

### Role Permissions

| Action | Customer | Staff | Admin |
|--------|----------|-------|-------|
| View class schedule | yes | yes | yes |
| Book a spot (self) | yes | yes | yes |
| Cancel own booking | yes | yes | yes |
| View own balance/history | yes | yes | yes |
| Book for others (walk-in) | — | yes | yes |
| Cancel any booking | — | yes | yes |
| Card ops (activate, top-up, block) | — | yes | yes |
| Process payments (debit/credit) | — | yes | yes |
| Cancel a class occurrence | — | yes | yes |
| Manage class templates | — | — | yes |
| Manage services/pricing | — | — | yes |
| Manage users/roles/settings | — | — | yes |

## Features — Phase 1 (Legacy Replacement)

### Customer-Facing (PWA)
1. **Register/Login** — email+password, Google/Facebook OAuth
2. **Link barcode card** — enter card number to connect legacy card to digital account
3. **View class schedule** — this week's classes with instructor and availability (e.g., "Mon 17:00 — Judita — 5/10 booked")
4. **Book a spot** — tap a class → confirm → spot reserved (enforces capacity limit)
5. **My bookings** — list upcoming bookings with cancel option
6. **My balance** — current credit and transaction history (if card linked)

### Staff-Facing
7. **Class dashboard** — today's and upcoming classes with participant names
8. **Walk-in booking** — add a customer to a class on their behalf
9. **Cancel any booking** — remove a participant from a class
10. **Cancel class occurrence** — cancel a specific date's class (notifies all booked users via UI)
11. **Card operations** — enter barcode → activate new card, top up credit, block/unblock
12. **Process payment** — select customer/card → select service (Spinning/Fitness) → debit credit
13. **Storno (refund)** — credit back to customer

### Admin
14. **Class template management** — create/edit/delete recurring weekly class schedule
15. **Instructor management** — add/edit/deactivate instructors
16. **Service management** — add/edit services with default prices
17. **User management** — view users, change roles, block/unblock
18. **Settings** — bike count (capacity), center name
19. **Data migration** — CLI tool to one-time import from legacy Access DB

### Live Updates
20. **WebSocket** — booking count updates in real-time across all connected clients

## UI Layout

Single responsive layout for all roles — **slot cards** showing classes with availability.

**Customer view:**
- Class cards with: time, instructor, availability count, BOOK/FULL/BOOKED status
- Booked classes show Cancel option
- Day picker for navigating the week

**Staff view (same layout + extras):**
- Participant name tags visible on each class card
- "+ Walk-in" button per class
- "✕" to cancel individual bookings
- "+ Assign" on empty slots to schedule classes
- Access to card operations and payment processing via navigation

**Color coding:**
- Green: available class (has spots)
- Blue: user's own booking
- Red: full class
- Grey: no class scheduled / cancelled

## Data Migration (One-Time)

Import from legacy Access DB (`db.mdb`) via CLI command:

1. **Cards:** 76 records → `cards` table (barcode, credit in EUR, blocked status, allow_debit)
2. **Transactions:** 1164 records → `transactions` table (action, amount EUR, date)
3. **Instructors:** 6 records → `instructors` table
4. **Services:** Spinning + Fitness → `services` table
5. **Staff accounts:** Created fresh with new hashed passwords (legacy had plaintext)
6. **Drop:** Slovak Koruna amounts, no-show tracking, FTP sync, receipt printing, squash/sauna/pingpong services

Cards are imported as unlinked (user_id = NULL). Existing members link their card to a new digital account after registering.

## Phase 2 (Future Extensions)

Not in scope for Phase 1, documented for reference:
- Push notifications for class reminders
- Online credit top-up (payment gateway integration)
- Attendance tracking (check-in at reception)
- Usage statistics dashboard
- Receipt export/email
- Multi-center support
