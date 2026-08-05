# Frontend split and multi-product strategy

## Branch model

| Branch | Purpose |
| --- | --- |
| `main` | Full-stack baseline (monorepo originale) |
| `frontend/split` | Frontend separato dal backend; base per prodotti indipendenti |

Su `frontend/split` il client Sprout vive in `frontend/sprout-web/`. Il backend resta in `apps/server/` e `crates/`.

## Obiettivo

Costruire **più software diversi**, ciascuno con il proprio stack (frontend + backend dedicati). Non esiste un unico server condiviso tra tutti i prodotti.

Sprout resta il **primo prodotto** (task platform E2EE). I prossimi gestionali nasceranno come repository o cartelle separate.

## Layout attuale (`frontend/split`)

```
sprout/
├── apps/server/              # Backend Sprout (API Axum)
├── frontend/
│   └── sprout-web/           # PWA React del primo prodotto
├── crates/                   # Librerie Rust condivise (per Sprout)
├── db/                       # Schema PostgreSQL di Sprout
└── docs/
```

## Aggiungere un nuovo prodotto

1. Creare una cartella `frontend/<nome-prodotto>/` oppure un **repository GitHub separato**.
2. Definire backend e API del nuovo software in un repo dedicato (non riutilizzare `apps/server` se il dominio è diverso).
3. Condividere solo ciò che ha senso (es. pattern di build WASM, template Vite) — non il codice domain-specific di Sprout.

## Estrarre un frontend in un repo standalone

Quando un prodotto è maturo:

```sh
# Dal monorepo
git subtree split -P frontend/sprout-web -b sprout-web-only

# Nel nuovo repo
mkdir ../sprout-web && cd ../sprout-web
git init
git pull ../sprout sprout-web-only
git remote add origin git@github.com:abaco-click/sprout-web.git
git push -u origin main
```

Il nuovo repo dovrà includere o dipendere da:

- tipi/contratti API del **proprio** backend
- build WASM se usa crittografia client-side
- CI frontend autonoma

## Sviluppo frontend Sprout (questo branch)

```sh
npm --prefix frontend/sprout-web install
npm --prefix frontend/sprout-web run wasm:build
npm --prefix frontend/sprout-web run dev
```

Il backend va avviato separatamente da `apps/server` con PostgreSQL configurato (vedi `.env.example`).
