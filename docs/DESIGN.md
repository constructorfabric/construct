# Technical Design — Construct (Fabric)

<!-- toc -->

- [1. Architecture Overview](#1-architecture-overview)
  - [1.1 Architectural Vision](#11-architectural-vision)
  - [1.2 Architecture Drivers](#12-architecture-drivers)
  - [1.3 Architecture Layers](#13-architecture-layers)
- [2. Principles & Constraints](#2-principles--constraints)
  - [2.1 Design Principles](#21-design-principles)
  - [2.2 Constraints](#22-constraints)
- [3. Technical Architecture](#3-technical-architecture)
  - [3.1 Domain Model](#31-domain-model)
  - [3.2 Component Model](#32-component-model)
  - [3.3 API Contracts](#33-api-contracts)
  - [3.4 Internal Dependencies](#34-internal-dependencies)
  - [3.5 External Dependencies](#35-external-dependencies)
  - [3.6 Interactions & Sequences](#36-interactions--sequences)
  - [3.7 Database schemas & tables](#37-database-schemas--tables)
  - [3.8 Deployment Topology](#38-deployment-topology)
- [4. Additional context](#4-additional-context)
- [5. Traceability](#5-traceability)

<!-- /toc -->

**ID**: `cpt-cf-construct-design`

> Status: **draft for review** (graph reframe, 2026-08-11). Supersedes the
> pre-graph Construct architecture. Written for the Insight/Studio graph session
> and the Friday roadmap review; captures decisions and open questions from the
> 2026-08-11 design call. This is the Rust, Fabric-native Construct
> (`constructorfabric/construct`); it is a distinct system from the legacy Python
> construct (`rolos` / hub `docs/construct/`, prefix `cpt-roloscyber-construct-*`).

## 1. Architecture Overview

### 1.1 Architectural Vision

Construct is an **engine that projects an abstract fact graph into a structured
profile**. It gathers every fact it can about an entity — a person, a product,
an organization — records the relationships between those facts as a graph, and
lets a caller run a query template against that graph to receive a structured
profile. *Developer? Here is the query. Learner? Here is the query. In return you
get the structured profile.*

Construct is **not a hard-coded product**; it is an engine on which owners build
their own constructs. A construct owner cannot be anticipated a priori — a
software company wants developer profiles built from commits and message tone; a
law firm wants every contract and email retained in full. Construct ships an
engine plus a small set of ready templates (software-company developer, learner,
researcher) and lets owners define the rest.

Three properties define the product: an **open-source, alienable core** (owners
enrich it with paid differentiators, and an owner who leaves takes their data
plus a minimal runnable engine); a construct is a **`(schema, extraction algorithm)` pair, versioned in parallel** over the same data; and it is a
**weighted projection, not exhaustive collection**. Construct, Insight and Studio
are three projections of one problem: gather information about entities, then
present it as action items (Insight), an analytical view (Studio), or an
explanatory profile (Construct).

### 1.2 Architecture Drivers

The one open architecture decision is the graph storage engine —
`cpt-cf-construct-adr-graph-storage-engine`
([ADR-0001](ADR/0001-graph-storage-engine.md)).

#### Functional Drivers

- Describe an entity type once, then **compare entities of that type** (find
  product competitors; transfer best-practice processes between organizations;
  assess a developer).
- Collect facts from **many heterogeneous connectors**, where the same attribute
  may arrive from several sources in conflicting formats.
- Produce a **structured, explanatory profile** answering qualitative questions
  ("how strong a developer", "why did output drop"), not just quantitative
  trends.
- Support **context subgraphs**: the same entity projected differently per
  audience (colleagues vs friends vs instructors).
- Let owners define **custom construct templates**; a template marketplace is a
  possible future (out of scope for v1).

#### NFR Allocation

| NFR                                 | Allocation                                                                                                                                                                                                                                                                                                         |
| ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Alienability / portability          | Storage + engine must allow an owner to export data and run a minimal engine standalone (§2.2, [ADR-0001](ADR/0001-graph-storage-engine.md)).                                                                                                                                                                      |
| Tenant isolation, governance, audit | Every operation mediated by the access model; reuse existing Rust libraries (§3.3, §3.4).                                                                                                                                                                                                                          |
| Cost control                        | Construct logic must hit a chosen price point ($5 good-enough vs $5000 perfect) without burning tokens (§3.2 L4).                                                                                                                                                                                                  |
| Scale                               | ~100s of millions of objects, ~500M edges per construct; billions aspirational — depth-bounded, not deep-traversal ([ADR-0001](ADR/0001-graph-storage-engine.md)).                                                                                                                                                 |
| Privacy / PII governance            | Construct ingests personal data (messages, tone, social, HR/AD). Every attribute carries a data-category tag; collection is purpose-limited and retained per tenant policy; the access model must support subject-level `anonymize`, `extract` and erasure (§2.2 `cpt-cf-construct-constraint-privacy-pii`, §3.3). |
| Security                            | Every operation authenticated and authorized through the platform access model (CModel + JWT middleware); tenant isolation and audit reused from the Rust libraries (§3.3, §3.8). A dedicated threat-model pass is deferred to the security review gate, not to this DESIGN.                                       |
| Reliability                         | Layer-4 workflows resumable from checkpoint on the durable runtime; ingestion idempotent per `(connector, provenance, version)` so replays do not duplicate facts (§3.6). Numeric availability/latency SLOs are deferred to DECOMPOSITION.                                                                         |

### 1.3 Architecture Layers

The engine is four layers. The first three are "dumb but robust" infrastructure;
the fourth is where product value and differentiation live.

| #   | Layer                        | Purpose                                                                                           | Sourcing                                                    |
| --- | ---------------------------- | ------------------------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| 1   | **GTS contracts**            | Global Type System: schema, validation, per-attribute provenance                                  | Reused from Gears/Fabric                                    |
| 2   | **Connectors**               | Integrations that collect facts and bring their own schema                                        | Some reused from Insight, some written per source           |
| 3   | **Graph store**              | Property graph of nodes/edges/weighted-properties; validates contracts on insert; stores at scale | See [ADR-0001](ADR/0001-graph-storage-engine.md)            |
| 4   | **Construct-logic workflow** | The "art": query the graph, summarise under cost/quality budget, emit the versioned construct     | Built here; candidate for Fabric serverless durable runtime |

## 2. Principles & Constraints

### 2.1 Design Principles

#### Engine, not product

**ID**: `cpt-cf-construct-principle-engine-not-product`

No construct scenario is hard-coded; owners compose their own from templates,
connectors and contracts.

#### Collector / interpreter separation

**ID**: `cpt-cf-construct-principle-collector-interpreter-split`

Those who *collect* data (connectors) and those who *interpret* it (construct
logic) are different actors with different needs. The GTS contract sits between
them as the enforcement point — the "policeman" that rejects data an interpreter
could not use (e.g. a phone number with no leading `+`).

#### Provenance is intrinsic

**ID**: `cpt-cf-construct-principle-intrinsic-provenance`

Every attribute records which connector, vendor, package, type and version
produced it. There is no bare "the phone number" field; there are per-source,
versioned values.

#### Weights are first-class

**ID**: `cpt-cf-construct-principle-weights-first-class`

Nodes, edges and properties carry weights so the logic layer can focus on the
important and ignore the rest — the core distinction from Insight.

#### Reuse the platform

**ID**: `cpt-cf-construct-principle-reuse-platform`

Tenant isolation, auth, governance, audit and the datastore come from existing
Rust libraries and the Model Engine platform, not bespoke implementations.

#### Reversible storage

**ID**: `cpt-cf-construct-principle-reversible-storage`

The logic layer addresses the graph through an interface, so the engine can be
swapped (see [ADR-0001](ADR/0001-graph-storage-engine.md)).

### 2.2 Constraints

#### Language and runtime

**ID**: `cpt-cf-construct-constraint-rust-postgres`

Rust; platform-standard async PostgreSQL access (no ORM), per the Model Engine
`_platform` contracts, for **operational/platform data**. This constraint does
**not** predecide the **graph-store** engine (§3.2 L3), which is open in
[ADR-0001](ADR/0001-graph-storage-engine.md): if that ADR selects a non-Postgres
engine (e.g. reusing NebulaGraph `graph-storage`), this constraint continues to
govern only the platform datastore, not the graph tier.

#### Reused platform libraries

**ID**: `cpt-cf-construct-constraint-reuse-rust-libs`

Tenant isolation, auth, governance and audit are provided by existing Rust
libraries and must not be re-implemented.

#### GTS adopted as-is

**ID**: `cpt-cf-construct-constraint-gts-as-is`

Construct adopts the Global Type System from Gears/Fabric and does not define its
own type system.

#### Alienability

**ID**: `cpt-cf-construct-constraint-alienability`

The storage and engine choice must let an owner export data and run a minimal
engine standalone.

#### Heavy properties live in the side store

**ID**: `cpt-cf-construct-constraint-side-store`

Large properties/documents live in the existing bronze/silver/gold layers, not
inline in the graph.

#### Vector search beside the graph

**ID**: `cpt-cf-construct-constraint-vector-beside-graph`

A vector capability sits beside the graph; an LLM relevance judge sits above raw
vector search, because vector search matched semantic similarity but not meaning.

#### Privacy and PII governance

**ID**: `cpt-cf-construct-constraint-privacy-pii`

Construct ingests personal data about identifiable people, so privacy is a
first-class constraint, not an afterthought. Every attribute carries a
data-category classification; collection is purpose-limited and retained under a
per-tenant policy; and the access model must support subject-level `anonymize`,
`extract` and hard erasure ("right to be forgotten"). This is enforced through the
reused governance/audit libraries (`cpt-cf-construct-constraint-reuse-rust-libs`),
not bespoke code. Erasure interacts with the still-open `write`/`delete` operation
set (§3.3) — deletion propagation to the bronze/silver/gold side store is an open
item, flagged here so it is not lost.

## 3. Technical Architecture

### 3.1 Domain Model

Core graph primitives (property-graph model):

- **Node** — a typed object: a person, product, organization, email, pull
  request, social post. Everything a person or company produces is a node.
- **Edge** — a typed, directed, weighted relationship (`manages`, `authored`,
  `owns`, `member-of`, `mentions`), connecting same-typed nodes (person →
  manager) or different-typed nodes (person → pull-request).
- **Property** — an attribute of a node or edge, stored *separately*, carrying
  value + weight + GTS provenance (vendor/package/type/version + connector).
  Large properties (documents) live in the side store, referenced by ID.

Higher-level concepts: a **Construct** is `(schema, extraction algorithm, version)`, many versions coexisting over one graph; a **Context subgraph** is a
named subset of an entity's graph for a given audience (the "graph of
constructs" is the network of links between constructs — a step toward a social
graph); a **Template** bundles a construct schema/algorithm plus the connectors
and bronze-layer contracts it depends on.

### 3.2 Component Model

```
Connectors ──▶ [GTS validate] ──▶ Graph Store ──▶ Construct-Logic Workflow ──▶ Construct output
 (L2)               (L1)             (L3)                  (L4)                 (structured profile)
    │                                  │                      │
    └── bring schema            nodes/edges/            query language +
        + provenance            weighted props          weighted summarisation
                                   │                      │
                        properties/documents ──▶ bronze/silver/gold side store
```

#### GTS Contracts (L1)

**ID**: `cpt-cf-construct-component-gts`

##### Why this component exists

The same entity/attribute is inserted by many connectors in conflicting formats;
without a validated contract, interpreters downstream cannot rely on the data.

##### Responsibility scope

Define schemas + versions (vendor/package/type/version), validate every insert
against the declaring connector's contract, attach per-attribute provenance.

##### Responsibility boundaries

Does not collect data (connectors do) and does not interpret it (construct logic
does); it only defines and enforces contracts. Reused from Gears/Fabric as-is.

##### Related components (by ID)

Enforced at the boundary of `cpt-cf-construct-component-graph-store`; populated by
`cpt-cf-construct-component-connectors`.

#### Connectors (L2)

**ID**: `cpt-cf-construct-component-connectors`

##### Why this component exists

Facts live in external systems (M365, GitHub, Zulip, social, HR/AD); something
must collect them and map them onto GTS schemas.

##### Responsibility scope

One connector per source, written roughly once; each brings its own schema,
declares what it enriches, and inserts validated data.

##### Responsibility boundaries

Does not decide what is important (no weights) and does not store data; hands
validated records to the graph store. Some connectors are reused from Insight.

##### Related components (by ID)

Produces data validated by `cpt-cf-construct-component-gts` and stored by
`cpt-cf-construct-component-graph-store`.

#### Graph Store (L3)

**ID**: `cpt-cf-construct-component-graph-store`

##### Why this component exists

The nodes/edges/weighted-properties must be stored at scale, validated on insert,
and served under depth-bounded weighted queries.

##### Responsibility scope

Persist nodes/edges/properties efficiently and securely; validate contracts on
insert; serve two-phase depth-bounded queries; enforce tenant isolation and
audit via the reused Rust libraries.

##### Responsibility boundaries

Does not run construct logic; "dumb but robust". Heavy properties/documents are
delegated to the bronze/silver/gold side store. Engine choice is open — see
[ADR-0001](ADR/0001-graph-storage-engine.md).

##### Related components (by ID)

Consumes contracts from `cpt-cf-construct-component-gts`; queried by
`cpt-cf-construct-component-construct-logic`.

#### Construct-Logic Workflow (L4)

**ID**: `cpt-cf-construct-component-construct-logic`

##### Why this component exists

Turning a graph into a qualitative, cost-bounded profile is the product's
differentiator and its "art".

##### Responsibility scope

Query the graph, fetch details on demand, summarise under a cost/quality budget,
emit and store the versioned construct; support many construct versions over one
graph.

##### Responsibility boundaries

Does not own storage or contracts; addresses the graph only through its
interface. Candidate host: the Fabric serverless durable runtime.

##### Related components (by ID)

Queries `cpt-cf-construct-component-graph-store`; emits constructs governed by
the access model (§3.3).

### 3.3 API Contracts

Filesystem analogy — Construct exposes **data**, **operations over data**, an
**access/governance model**, and a **graph of constructs**. Operation set (to be
finalised — flagged by Ivan in the call):

- `read`, `share`, `anonymize`, `transfer`, `extract`, `compare` (compare two
  constructs of the same type).
- `write` / `delegate-write` — **missing from the current draft, to be added.**
- `subscribe` / `notify` — event when a construct changes, so subscribers react.
  **To be added.**

Every operation is mediated by the access model: *who* may perform *which*
operation on *which* data category under *which* permissions, with an audit
(access-log) entry.

### 3.4 Internal Dependencies

- **GTS** (Gears/Fabric) — contract/type system, layer 1.
- **Existing Rust libraries** — tenant isolation, auth, governance, audit.
- **bronze/silver/gold storage** (Insight lineage) — raw/refined document store
  beside the graph.
- **Insight `graph-storage` (NebulaGraph)** — candidate reuse for layer 3 (see
  [ADR-0001](ADR/0001-graph-storage-engine.md)).
- **Vector store** (engine undecided — pgvector vs Qdrant, see
  [ADR-0001](ADR/0001-graph-storage-engine.md)) — semantic similarity beside the
  graph, sitting under the LLM relevance judge.
- **Fabric serverless durable runtime** — candidate host for layer 4 workflows
  (resumable from checkpoint, orchestrated so only N run per entity).

### 3.5 External Dependencies

#### Source systems (via connectors)

Microsoft 365, GitHub, Zulip, social networks (Facebook/X), HR / Active
Directory, and other per-source integrations.

#### LLM providers

Extraction, summarisation and the relevance judge, across cost tiers (cheap vs
expensive models, shallow vs deep content analysis).

### 3.6 Interactions & Sequences

#### Ingestion (connector → graph)

**ID**: `cpt-cf-construct-seq-ingestion`

1. A connector collects facts and maps them to its declared GTS schema (attaching
   vendor/package/type/version + connector provenance).
1. It inserts into the graph store; GTS validates the contract; violations are
   rejected, valid data is written as nodes/edges/properties; large properties go
   to the side store and are referenced by ID.

#### Construct projection (query → profile)

**ID**: `cpt-cf-construct-seq-projection`

1. The logic layer issues a depth-bounded query ("everything about entity X to
   depth N") and receives a list of typed IDs.
1. It requests details for a weight-ranked subset (e.g. the top 10) from the side
   store.
1. It summarises under a cost/quality budget (reusing prior summaries), writes the
   summary back as a derived fact, and assembles the final structured construct.
1. The construct is stored; multiple versions may run this pipeline differently
   over the same graph.

### 3.7 Database schemas & tables

Logical schema (provisional — physical form depends on
[ADR-0001](ADR/0001-graph-storage-engine.md)). Graph tables stay lean (IDs,
edges, weights); heavy values live in the side store.

#### Table: nodes

Typed objects. Columns (logical): `id`, `type`, `tenant_id`, `weight`,
`created_at`. Isolation via the reused Rust libraries (tenant-scoped).

#### Table: edges

Typed, directed, weighted relationships. Columns (logical): `id`, `type`,
`src_id`, `dst_id`, `tenant_id`, `weight`.

#### Table: properties

Node/edge attributes with provenance. Columns (logical): `id`, `owner_id`,
`key`, `value_ref` (inline scalar or side-store document reference), `weight`,
`vendor`, `package`, `type`, `version`, `connector`, `tenant_id`.

### 3.8 Deployment Topology

Follows the Model Engine platform baseline (PostgreSQL; platform middleware for
CModel + JWT; structured logging; health/ready/version endpoints). Layer 4 may
deploy on the Fabric serverless durable runtime. **Detail pending the storage and
runtime decisions.**

## 4. Additional context

**Distinction from Insight.**

|            | Insight                              | Construct                             |
| ---------- | ------------------------------------ | ------------------------------------- |
| Collection | everything, link everything          | weighted; prune to what matters       |
| Output     | trends / quantitative, no conclusion | qualitative verdict + explanation     |
| Traversal  | operational, exhaustive              | weighted summarisation, depth-bounded |
| Weights    | none                                 | on nodes, edges, properties           |

**Open questions** (agenda for the Insight/Studio session and Friday review):

1. **Graph engine** — reuse NebulaGraph `graph-storage` vs Postgres+pgvector+graph
   layer ([ADR-0001](ADR/0001-graph-storage-engine.md)); vector store pgvector vs
   Qdrant (given the `document-storage` precedent).
1. **Output shape** — confirmed a **structured/hybrid tree**, not a flat fact
   list: a hierarchy where each node carries required properties (category,
   source/provenance, applicable context). Required-property schema to finalise.
1. **Operation set** — add `write`/`delegate-write` and `subscribe`/`notify`
   (Ivan); finalise the list and permission mapping.
1. **Facts vs graph queries** — a construct as typed facts, a query result
   document (JSON/table), or a hybrid? Working answer: a hybrid document.
1. **Template scope** — which role templates ship in v1 (developer, learner,
   researcher); lawyer and others deferred; marketplace out of scope for v1.
1. **Workflow runtime** — plain Python-over-graph for v1 vs Fabric serverless
   durable runtime; orchestration limits (≤ N concurrent workflows per entity).
1. **Insight infra reliance** — can Construct rely on the Insight team's graph
   infrastructure, or must it own the infra? Affects roadmap and query complexity.

## 5. Traceability

- **[ADR-0001: Graph Storage Engine](ADR/0001-graph-storage-engine.md)**
  (`cpt-cf-construct-adr-graph-storage-engine`) — realises §3.2 (Graph Store) and
  §3.7 / layer 3.
- Internal chain: the principles (§2.1) and constraints (§2.2) realise the drivers
  in §1.2; the components (§3.2) and sequences (§3.6) realise the layers in §1.3.
  The privacy/PII constraint (`cpt-cf-construct-constraint-privacy-pii`) and the
  Privacy/PII, Security and Reliability NFRs (§1.2) are realised by the access
  model in §3.3 and the reused governance libraries.
- Downstream (to author): PRD (PM-owned, in progress), the AGE-vs-CTE follow-up
  ADR, DECOMPOSITION, and per-layer FEATURE docs.
- Source: graph-reframe design call, 2026-08-11.
