---
status: proposed
date: 2026-08-11
decision-makers: Yehor Komarov (design owner)
consulted: Vasyl Shandyba (Insight graph-storage), Alexander Andreev, Anastasia Berseneva, Ivan Lukovnikov
informed: Constructor Fabric platform team
---

# ADR-0001: Graph Storage Engine for the Construct Projection

<!-- toc -->

- [Context and Problem Statement](#context-and-problem-statement)
- [Decision Drivers](#decision-drivers)
- [Considered Options](#considered-options)
- [Decision Outcome](#decision-outcome)
  - [Consequences](#consequences)
  - [Confirmation](#confirmation)
- [Pros and Cons of the Options](#pros-and-cons-of-the-options)
  - [Reuse the Insight graph-storage (NebulaGraph)](#reuse-the-insight-graph-storage-nebulagraph)
  - [PostgreSQL + pgvector + a graph layer](#postgresql--pgvector--a-graph-layer)
  - [Standalone native graph database](#standalone-native-graph-database)
  - [Rejected without scoring: tree-sitter](#rejected-without-scoring-tree-sitter)
- [More Information](#more-information)
- [Traceability](#traceability)
- [Review Cadence](#review-cadence)

<!-- /toc -->

**ID**: `cpt-cf-construct-adr-graph-storage-engine`

## Context and Problem Statement

Construct is an engine that projects an abstract fact graph into a structured
profile. Connectors collect facts about an entity (person, product,
organization) from many sources; the facts and their relationships are stored
as a graph of nodes, edges and properties; and a versioned `(schema, extraction algorithm)` pair — a *construct* — traverses that graph under a query language
to emit a qualitative profile ("is this a strong front-end developer", "why did
this person's output drop"). Construct is deliberately **not** Insight: Insight
collects everything and links everything with no notion of importance, whereas
Construct puts **weights** on nodes, edges and properties and summarises *by
weight*, pruning to the few facts that matter rather than traversing every edge.

The graph is the one component of the four-layer architecture (GTS contracts →
connectors → graph store → construct-logic workflow) that does not yet exist and
whose technology is unsettled. Everything downstream — the query language the
construct logic can express, achievable latency, tenant-isolation story, and the
roadmap itself — depends on this choice, so it must be settled before the
Construct roadmap can be committed. There is no external deadline on the
technology; the pressure is sequencing: the construct-logic work stays blocked
until the store exists.

Which storage engine class should back the Construct graph? The store must:

- model a **property graph** — typed nodes, typed edges, and properties that may
  carry per-attribute provenance (which connector, which vendor/package/type/
  version produced this value; see GTS);
- validate contracts **on insert** and reject malformed data (the "policeman"
  role) transactionally;
- answer **depth-bounded, weighted** queries of the form "everything about entity
  X to depth N → a list of IDs", after which the caller fetches heavy
  properties/documents from a side store — not deep unbounded traversal;
- carry **large properties/documents in a separate store** (the existing bronze/
  silver/gold layers), keeping the graph tables lean (IDs, edges, weights);
- enforce **tenant isolation, access governance and audit**, reusing the
  platform's existing Rust libraries rather than re-implementing them;
- support **semantic relevance** over facts (vector similarity), noting that an
  LLM relevance judge already sits above raw vector search because vector search
  caught *semantic* similarity but not *meaning*;
- honour **alienability** — the core engine is open-source and a construct owner
  who stops paying must be able to leave with their data plus a minimal,
  embeddable engine, so the store must be portable and open, not a hosted
  proprietary service.

The scale figure discussed ("hundreds of millions of objects, ~500M edges per
construct; billions aspirational") is real but is *not* a deep-traversal
workload: the weighted-prune design specifically avoids walking large
neighbourhoods.

## Decision Drivers

- **Reuse mandate (highest weight).** Tenant isolation, auth, governance and
  audit are provided by existing Rust libraries built around the platform's
  standard datastore (PostgreSQL, async driver, no ORM — see the Model Engine
  `_platform` contracts). One datastore means one isolation story and one audit
  log; a second operational engine reopens all three.
- **Alienability.** Favours an embeddable, open, dumpable store; disfavours a
  hosted/proprietary graph service.
- **Transactional contract validation.** GTS check-on-insert wants a real
  transaction spanning the write.
- **Query shape is depth-bounded and weighted, not deep-traversal.** The single
  place a native graph engine decisively beats a relational one — many-hop
  traversal over billions of edges — is a workload this design avoids by
  construction.
- **Semantic relevance is required anyway.** A vector capability (plus the LLM
  relevance judge above it) must live beside the graph.
- **Privacy / erasure.** The store holds personal data and must support
  subject-level anonymization and hard deletion (right-to-erasure) transactionally
  and auditably; this favours a store with straightforward row-level delete + audit
  over an append-oriented or eventually-consistent engine.
- **Operational simplicity and time-to-first-construct.** v1 needs a running
  store this quarter, not a new distributed system to operate.
- **Reversibility.** Whatever is chosen, the construct logic must address the
  store through an interface (`nodes / edges / weighted-properties`, two-phase
  depth query, GTS-validated insert) so the engine can be swapped without
  rewriting construct logic.

## Considered Options

1. **Reuse the Insight `graph-storage` service (NebulaGraph).** The Insight/
   Hypothesis platform already ships a `graph-storage` service backed by
   NebulaGraph, a native distributed graph database, with the most in-house graph
   expertise (Vasyl Shandyba and the Singapore team). Adopt or extend it.
1. **PostgreSQL + pgvector + a graph layer.** One Postgres instance holds nodes/
   edges/weights and pgvector embeddings; the graph query surface is provided by
   a graph layer over Postgres. The concrete graph layer — Apache AGE (openCypher
   in Postgres) versus hand-written recursive CTEs — is **deferred to a follow-up
   ADR**; this ADR selects only the engine *class*.
1. **Standalone native graph database** (Neo4j, Memgraph, or a fresh NebulaGraph
   instance dedicated to Construct), paired with a separate vector store.
1. **Rejected without scoring: tree-sitter.** Raised as "a compact query language
   over a tree". Excluded as a category error (see below).

## Decision Outcome

**Status: proposed — decision pending the Insight/Studio graph session.**

Chosen leaning: **Option 2, PostgreSQL + pgvector + a graph layer**, placed behind
a storage interface so the choice is reversible — *pending* the working session
with the Insight team (Vasyl Shandyba, Andrey Kuchma, Nikita Derevsky), which may
redirect us to **Option 1 (reuse NebulaGraph graph-storage)** if that service is
multi-tenant-capable, embeddable enough to satisfy alienability, and reusable
without standing up a second operational system for Construct.

Rationale for the leaning: Option 2 satisfies four drivers at once — the reuse
mandate (one Postgres, one tenant-isolation/audit story via the existing Rust
libraries), alienability (Postgres is open, embeddable and dumpable), transactional
GTS validation, and the co-located vector capability the design needs regardless —
while the one workload where a native graph engine wins (deep multi-hop traversal)
is precisely what the weighted-prune design avoids. The reversibility escape hatch
is what makes it safe to commit to Postgres for v1 without over-analysing scale we
do not yet have.

Two internal precedents must be reconciled and are the reason this stays
`proposed`, not `accepted`:

- Vasyl's shipped graph work uses **NebulaGraph**, not pgvector. The "Postgres +
  pgvector" recommendation attributed to him must be confirmed as either (a) an
  evolved position, (b) a *layered* design (NebulaGraph for edges + pgvector for
  embeddings), or (c) a mis-recollection.
- The sibling Model Engine `document-storage` service chose **Qdrant over
  pgvector** for embeddings (its ADR-0001, accepted 2026-04-03). If Construct
  standardises on pgvector, this ADR must say why Construct diverges from that
  precedent; if it does not, "pgvector" in Option 2 becomes "Qdrant beside
  Postgres".

### Consequences

- Good: single datastore for v1; existing Rust isolation/auth/governance/audit
  reused unchanged; transactional inserts; alienability preserved; vector search
  co-located; construct logic written against a stable interface, not a vendor.
- Bad / risks: Postgres graph traversal degrades earlier than a native engine if
  the workload ever becomes deep-traversal-heavy (mitigated by the weighted-prune
  design and the storage interface); the AGE-vs-CTE sub-decision is still open;
  divergence from the `document-storage` Qdrant precedent needs an explicit
  justification or a switch to Qdrant.
- Neutral: heavy properties/documents continue to live in the bronze/silver/gold
  layers; the graph tables stay lean.

### Confirmation

1. Hold the Insight/Studio graph session; record whether `graph-storage`
   (NebulaGraph) is reusable under our tenant-isolation and alienability
   constraints. If yes, this ADR flips to Option 1.
1. Confirm Vasyl's actual recommendation and whether the design is layered
   (graph engine + separate vector store).
1. Resolve the vector store against the `document-storage` Qdrant precedent.
1. Run a bounded spike against a **measurable pass/fail gate** (numbers
   provisional, to be ratified in the session): on a synthetic graph of ~100M
   nodes / ~500M edges on a single primary Postgres node, the "entity → depth N →
   IDs" query (depth ≤ 3, ≤ a few thousand candidate IDs) returns at **p95 ≤
   200 ms**, and a full weighted-summarisation projection completes at **p95 ≤ 2 s**
   excluding LLM time. Meeting the gate is a precondition for moving this ADR to
   `accepted`; missing it reopens Option 1 (NebulaGraph) or Option 3.
1. Open the follow-up ADR selecting Apache AGE vs recursive CTEs.

## Pros and Cons of the Options

### Reuse the Insight graph-storage (NebulaGraph)

- Good: native graph engine; strongest in-house expertise; genuine deep-traversal
  performance; no new graph engine to design.
- Good: avoids duplicated effort across Construct/Insight/Studio, which the call
  framed as three projections of one problem.
- Bad: a second operational datastore beside Postgres, so tenant isolation, auth,
  governance and audit must be re-established there rather than reused.
- Bad: alienability is hard — a distributed graph DB is not something a departing
  owner can trivially take and run embedded.
- Unknown: multi-tenant readiness and whether Construct's contract-on-insert and
  provenance model fit its schema; to be answered in the session.

### PostgreSQL + pgvector + a graph layer

- Good: reuses the platform-standard datastore and its Rust isolation/auth/
  governance/audit libraries unchanged; one store, one audit log.
- Good: open, embeddable, dumpable — satisfies alienability.
- Good: transactional GTS validation on insert; vector search co-located; matches
  the depth-bounded, weighted-prune query shape.
- Bad: weaker than a native engine for deep multi-hop traversal (a workload this
  design avoids); relies on the graph layer (AGE or CTEs) still to be chosen.
- Bad: pgvector diverges from the `document-storage` Qdrant precedent unless
  justified.

### Standalone native graph database

- Good: best raw traversal and a mature query language (Cypher).
- Bad: a separate operational system *and* a separate vector store; tenant
  isolation/governance not reused; licensing (Neo4j); weakest on alienability;
  heaviest to operate for a workload that does not need deep traversal.

### Rejected without scoring: tree-sitter

tree-sitter is an incremental **parser**: it builds a concrete syntax **tree**
from source text and matches S-expression patterns against that one in-memory
tree. It has no persistence, no indexing, no cross-record traversal, no
multi-writer or transactional semantics, and its "graph" is a strict tree (one
parent), not a weighted property graph. It solves parsing, not storage. The only
attractive property — a compact query DSL — is better obtained from openCypher
(via a graph layer) or GraphQL. Excluded as a category error to prevent it
resurfacing.

## More Information

- Model Engine `_platform` contracts (PostgreSQL, async driver, no ORM):
  `platform/aps/ai-engine/cyberrepo` → `architecture/_platform/DESIGN.md`.
- Vector-store precedent (Qdrant over pgvector):
  `platform/aps/ai-engine/cyberrepo` → `architecture/document-storage/ADR/0001-qdrant-vector-database.md`
  and `.../ADR/0004-dual-search-types.md`.
- Insight NebulaGraph `graph-storage` service: `gitops/apps-gitops` MRs under
  HYP-1887 ("set nebula graph space for graph-storage", "add ragApi.extraEnv for
  graph-storage").
- Design context: Construct DESIGN (this repo, `docs/DESIGN.md`) and the graph
  reframe discussion of 2026-08-11.
- GTS (Global Type System) contract layer, reused from Gears/Fabric.

## Traceability

- Realises the storage-layer decision required by Construct DESIGN §3 (Technical
  Architecture) and the graph-store component of the four-layer model.
- Downstream: the follow-up ADR on the graph query mechanism (Apache AGE vs
  recursive CTEs) and the vector-store confirmation (pgvector vs Qdrant).

## Review Cadence

Revisit once the Insight/Studio graph session and the Postgres spike complete
(target: within the current roadmap cycle), or immediately if the reuse of
`graph-storage` is confirmed — whichever comes first. Until then this ADR blocks
the Construct roadmap sign-off.
