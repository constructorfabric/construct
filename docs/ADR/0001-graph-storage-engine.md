---
status: proposed
date: 2026-08-12
decision-makers: Yehor Komarov (design owner)
consulted: Vasyl Shandyba (studio-graph-storage reference implementation), Alexander Andreev, Anastasia Berseneva, Ivan Lukovnikov
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
  - [Reuse the older Insight graph-storage (NebulaGraph)](#reuse-the-older-insight-graph-storage-nebulagraph)
  - [PostgreSQL + pgvector + Apache AGE mirror](#postgresql--pgvector--apache-age-mirror)
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

- model a **property graph** — typed nodes, typed edges with a weight and a
  confidence, and properties that may carry per-attribute provenance (which
  connector, which vendor/package/type/version produced this value; see GTS);
- validate contracts **on insert** and reject malformed data (the "policeman"
  role) transactionally;
- answer **depth-bounded, weighted** queries of the form "everything about entity
  X to depth N → a list of IDs", after which the caller fetches heavy
  properties/documents from a side store — not deep unbounded traversal;
- carry **large properties/documents in a separate store** (the existing bronze/
  silver/gold layers), keeping the graph tables lean (IDs, edges, weights);
- enforce **tenant isolation, access governance and audit**, reusing the Gears
  Toolkit secure data path rather than re-implementing them;
- support **semantic relevance** over facts — a vector capability beside the
  graph. Two relevance mechanisms are in play and their relationship is not yet
  settled: hybrid retrieval that fuses lexical + vector arms by reciprocal rank,
  and a small purpose-trained LLM relevance judge over a single entity's bounded
  fact set (see the Semantic-relevance driver below);
- honour **alienability** — the core engine is open-source, so a construct owner
  can leave someone else's deployment, redeploy the open-source engine and restore
  an export; the store must therefore be portable and open, not a hosted
  proprietary service.

The scale figure discussed ("hundreds of millions of objects, ~500M edges per
construct; billions aspirational") is real but is *not* a deep-traversal
workload: the weighted-prune design specifically avoids walking large
neighbourhoods.

**Reuse mode is decided:** we reuse the *architecture*, not the code. Vasyl's
`studio-graph-storage` is a Python reference implementation/demo; Construct
reimplements the store in **Rust as a Fabric gear**. This ADR selects the engine
*class* that Rust implementation targets.

## Decision Drivers

- **Reuse mandate (highest weight).** Tenant isolation, authn/authz, governance
  and audit are provided by the Gears Toolkit and the Fabric system gears, built
  around the platform-standard PostgreSQL accessed through the secure data path
  (`SecureConn` + `AccessScope` over SeaORM — Fabric *does* use an ORM). One
  datastore means one isolation story and one audit log; a second operational
  engine reopens all three.
- **Alienability.** Favours an embeddable, open, dumpable store; disfavours a
  hosted/proprietary graph service.
- **Transactional contract validation.** GTS check-on-insert wants a real
  transaction spanning the write.
- **Query shape is depth-bounded and weighted, not deep-traversal.** The single
  place a native graph engine decisively beats a relational one — many-hop
  traversal over billions of edges — is a workload this design avoids by
  construction.
- **Semantic relevance is required anyway.** A vector capability must live beside
  the graph. Whether the corpus-wide **hybrid RRF** (lexical + vector) of the
  reference implementation replaces or merely complements the **per-entity LLM
  relevance judge** (which operates over one person's bounded fact set, not
  millions) is an **open comparison** — to benchmark, not to assume.
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

1. **Reuse the older Insight `graph-storage` service (NebulaGraph).** The Insight/
   Hypothesis platform ships a `graph-storage` service backed by NebulaGraph, a
   native distributed graph database (HYP-1887). This is a **different, older**
   service from Vasyl's `studio-graph-storage` reference implementation. Adopt or
   extend it.
1. **PostgreSQL + pgvector + Apache AGE mirror.** One Postgres instance holds
   nodes/edges/weights/confidence in ordinary **relational tables (the source of
   truth)** plus pgvector embeddings; an **Apache AGE graph is a traversal mirror**
   dual-written in the same transaction, so it can be dropped without data loss.
   Search is hybrid: lexical (Postgres FTS) + vector (pgvector) fused by reciprocal
   rank; graph search seeds by name/vector then expands to depth N via Cypher in
   AGE and hydrates payloads from the relational tables. This is exactly the
   architecture of Vasyl's `studio-graph-storage` reference implementation.
1. **Standalone native graph database** (Neo4j, Memgraph, or a fresh NebulaGraph
   instance dedicated to Construct), paired with a separate vector store.
1. **Rejected without scoring: tree-sitter.** Raised as "a compact query language
   over a tree". Excluded as a category error (see below).

## Decision Outcome

**Status: proposed — recommended Option 2, pending the scale spike and the
Insight/Studio graph session as ratification.**

Chosen: **Option 2, PostgreSQL + pgvector + Apache AGE mirror**, placed behind a
storage interface so the choice is reversible. This is no longer a bet: it is the
architecture of a working reference implementation
(`vasylcf/studio-graph-storage`) built by the person this ADR consults.

Rationale: Option 2 satisfies four drivers at once — the reuse mandate (one
Postgres, one tenant-isolation/audit story via the Gears Toolkit secure data
path), alienability (Postgres is open, embeddable and dumpable; relational-as-
source-of-truth means AGE can be dropped with no data loss), transactional GTS
validation, and the co-located vector capability the design needs regardless —
while the one workload where a native graph engine wins (deep multi-hop traversal)
is precisely what the weighted-prune design avoids.

The reframe resolves the two precedents that previously kept this `proposed`:

- The "Vasyl uses NebulaGraph, not pgvector" discrepancy is gone. There are two
  different services: the *older* Insight `graph-storage` (NebulaGraph, Option 1)
  and Vasyl's *new* `studio-graph-storage` (Postgres + AGE + pgvector, Option 2).
  The Postgres+pgvector attribution was his current, implemented position.
- The **AGE-vs-CTE** sub-question is reframed: the reference implementation uses
  **AGE as a traversal mirror over authoritative relational tables**, which gives
  Cypher without conceding alienability or reversibility. A follow-up ADR may still
  weigh AGE-mirror vs CTE-over-relational.
- **pgvector vs Qdrant**: the reference implementation co-locates **pgvector** in
  the one Postgres, deliberately, for the single-store reuse/audit benefit. (The
  Qdrant precedent belongs to the legacy Model-Engine `document-storage` service,
  not Fabric, so it is context, not a binding precedent.)

What keeps it `proposed` rather than `accepted` is now narrow: the target-scale
spike is **not yet measured** (the reference implementation runs at demo scale,
~175 nodes / 635 edges), and the Insight/Studio session should ratify.

### Consequences

- Good: single datastore for v1; Gears Toolkit isolation/authz/governance/audit
  reused unchanged; transactional inserts; alienability preserved (relational
  source of truth, AGE droppable); vector search co-located; construct logic
  written against a stable interface, not a vendor; a working reference
  implementation to port from.
- Bad / risks: Postgres graph traversal degrades earlier than a native engine if
  the workload ever becomes deep-traversal-heavy (mitigated by the weighted-prune
  design and the storage interface); **Apache AGE targets PostgreSQL 16 only**
  (no PG17), a real ceiling mitigated by keeping AGE a swappable mirror; target
  scale is unproven and must be spiked.
- Neutral: heavy properties/documents continue to live in the bronze/silver/gold
  layers; the graph tables stay lean.

### Confirmation

1. Run the **scale spike** — the reference implementation demonstrates the
   architecture but only at demo scale; measure the two canonical queries ("entity
   → depth N → IDs" and a weighted-summarisation pass) at low-hundreds-of-millions
   of nodes / ~500M edges before moving to `accepted`. Provisional gate (to ratify
   in the session): the depth-bounded ID query at **p95 ≤ 200 ms** and a full
   projection at **p95 ≤ 2 s** excluding LLM time, on a single primary Postgres
   node. No numbers are asserted as passed yet.
1. Benchmark **hybrid RRF vs the per-entity LLM relevance judge** for relevance
   quality and latency; decide whether they are complementary.
1. Hold the Insight/Studio graph session as ratification (and to record whether
   the older NebulaGraph `graph-storage`, Option 1, has any residual pull).
1. Open the follow-up ADR on the traversal mechanism (AGE-mirror vs
   CTE-over-relational) and coordinate with
   [ADR-0002](0002-graph-construction-weighting-model.md) (construction/weighting).

## Pros and Cons of the Options

### Reuse the older Insight graph-storage (NebulaGraph)

- Good: native graph engine; genuine deep-traversal performance; no new graph
  engine to design.
- Good: avoids duplicated effort across Construct/Insight/Studio, which the call
  framed as three projections of one problem.
- Bad: a second operational datastore beside Postgres, so tenant isolation, authz,
  governance and audit must be re-established there rather than reused.
- Bad: alienability is hard — a distributed graph DB is not something a departing
  owner can trivially take and run embedded.
- Unknown: multi-tenant readiness and whether Construct's contract-on-insert and
  provenance model fit its schema.

### PostgreSQL + pgvector + Apache AGE mirror

- Good: reuses the platform-standard datastore and the Gears Toolkit secure data
  path unchanged; one store, one audit log; **a working reference implementation
  exists**.
- Good: open, embeddable, dumpable — satisfies alienability; relational tables are
  the source of truth, so the AGE mirror is droppable with no data loss.
- Good: transactional GTS validation on insert; hybrid lexical+vector search
  co-located; matches the depth-bounded, weighted-prune query shape.
- Bad: weaker than a native engine for deep multi-hop traversal (a workload this
  design avoids); **Apache AGE is PG16-only** (no PG17); target scale unproven.

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

- **Reference implementation:** `github.com/vasylcf/studio-graph-storage` —
  PostgreSQL 16 + pgvector + Apache AGE; relational source of truth + AGE traversal
  mirror; hybrid RRF search; GTS with a UUIDv5 `type_uuid` shared with the Rust
  implementation. Python/FastAPI demo; Construct reimplements the architecture in
  Rust.
- **Rank-fusion lineage:** the RRF fusion is ported from `studio-kit-hypothesis`
  (`_chunk_index_lib.py`).
- **GTS:** in-repo `libs/toolkit-gts` (crate `gts`) + the Types Registry gear; the
  UUIDv5 derivation vector is pinned in the account-management gear
  (`.../metadata/type_id.rs`).
- **Older Insight NebulaGraph `graph-storage`:** `gitops/apps-gitops` MRs under
  HYP-1887.
- **Legacy vector-store precedent (context only, not Fabric):** the Model-Engine
  `document-storage` service chose Qdrant over pgvector
  (`platform/aps/ai-engine/cyberrepo` → `architecture/document-storage/ADR/0001-qdrant-vector-database.md`).
- Design context: Construct DESIGN (this repo, `docs/DESIGN.md`) and the graph
  reframe discussion of 2026-08-11.

## Traceability

- Realises the storage-layer decision required by Construct DESIGN §3 (Technical
  Architecture) and the graph-store component of the four-layer model.
- Related: [ADR-0002](0002-graph-construction-weighting-model.md) (graph
  construction & weighting model) and DESIGN §3.9 (authorization) / §3.10 (data
  protection).
- Downstream: the follow-up ADR on the traversal mechanism (AGE-mirror vs
  recursive CTEs).

## Review Cadence

Revisit once the scale spike and the Insight/Studio graph session complete
(target: within the current roadmap cycle). Until the spike is measured this ADR
stays `proposed` and blocks the Construct roadmap sign-off.
