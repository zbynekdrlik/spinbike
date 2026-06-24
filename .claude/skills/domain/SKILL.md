---
name: spinbike-domain
description: >
  SpinBike domain knowledge: owner is solo operator (no role separation),
  cards are legacy fiction (users is the real entity), and brainstorm/spec
  verification discipline. Load before any design, spec, or feature work.
triggers:
  - design
  - spec
  - brainstorm
  - role
  - permission
  - card
  - barcode
  - chip
  - user
  - owner
  - Štefan
  - CEO
  - admin
---

# SpinBike Domain Knowledge

## Owner is solo operator — no role separation

SpinBike is run by a **single person**: **Štefan Sumerling** (`fitnescentrum.s.s@gmail.com`). He is simultaneously owner/CEO, admin, staff, and front desk. There is NO separate CEO role — the `admin` role IS Štefan.

**Design implications:**
- Do NOT propose new roles or permission tiers for "CEO vs staff" — they are the same person
- Every design decision should minimize Štefan's overhead (routine ops = few taps/clicks)
- Nav: task-mode ("at the desk now" vs "reviewing the day"), not role-based
- Reports: shaped around what HE wants to know daily, not corporate KPI dashboards
- No delegation features (e.g. "who did this transaction" — always Štefan)

## Cards are legacy fiction — `users` is the real customer entity

The legacy VB6+MS Access system used physical plastic cards (RFID/NFC; `cards.barcode` column is MISNAMED — the value is a chip code). In reality, **clients never received physical cards** — the "cards" only existed inside the legacy DB. CEO Štefan calls the dependence on card numbers an "unpleasant feature".

**How to apply:**
- Treat `users` as the canonical customer entity
- `cards.barcode` (chip code) is vestigial, not a primary identifier
- Search at the desk accepts name OR card code OR company; new accounts created as users, not cards
- Use "card code" (not "barcode") in any new UI copy
- Do NOT propose features that re-entrench chip codes as primary identifier (no "issue physical card" flows)
- Phase 2 (future): once card codes are no longer needed for legacy disambiguation, remove the column

## Brainstorm/spec: verify "existing flow already does this" claims

Before writing decisions like "uses existing X flow" or "existing handler already does this" — **grep the actual code** to confirm the handler accepts the assumed inputs (password fields, role checks, etc.).

**Anti-patterns that caused regressions:**
- Assuming `PUT /api/users/:id` had a password field (it didn't) → scope discovered only at PR-merge time
- Narrowing role scope tighter than the prompt stated (prompt said "users which is allowed" → spec silently narrowed to customer-only)

**Rules:**
1. Re-read the user's prompt verbatim. Words like "users", "everyone", "members" mean role-agnostic — do NOT silently narrow to one role unless the prompt explicitly distinguishes
2. Every "the existing X already does this" sentence in a spec is a verification-required claim → grep the module before writing it
3. When the spec lists an out-of-scope item, verify that the in-scope alternative has the wiring it claims

## Product context (always apply)

- Fitness center: ~6 spinning classes/week (2/day on 3 days), Squash Centrum Smižany, Slovakia
- Services: Spinning + Fitness only (dropped sauna, squash, refreshments)
- No individual bike assignment — just headcount against capacity
- Hybrid card system: legacy barcode codes + new digital accounts
- Hosted on Hetzner VPS (cloud), NOT local PC like the legacy app
