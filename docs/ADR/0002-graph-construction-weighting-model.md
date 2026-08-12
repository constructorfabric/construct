---
status: proposed
date: 2026-08-12
decision-makers: Yehor Komarov (design owner)
consulted: Vasyl Shandyba, Alexander Andreev, Anastasia Berseneva, Ivan Lukovnikov
informed: Constructor Fabric platform team
---

# ADR-0002: Graph Construction & Weighting Model

<!-- toc -->

- [Context and Problem Statement](#context-and-problem-statement)
- [Decision Drivers](#decision-drivers)
- [Considered Options](#considered-options)
- [Decision Outcome](#decision-outcome)
  - [Consequences](#consequences)
  - [Confirmation](#confirmation)
- [Pros and Cons of the Options](#pros-and-cons-of-the-options)
  - [Template-built weighted graph + query-time overrides](#template-built-weighted-graph--query-time-overrides)
  - [Single raw graph, all weights at query time](#single-raw-graph-all-weights-at-query-time)
  - [Hybrid: raw facts → construct-builder → base graph → construct-querier](#hybrid-raw-facts--construct-builder--base-graph--construct-querier)
- [More Information](#more-information)
- [Traceability](#traceability)

<!-- /toc -->

**ID**: `cpt-cf-construct-adr-graph-weighting-model`

## Context and Problem Statement

Construct projects a fact graph into a structured profile, and the projection is
**weighted** — importance is what separates it from Insight. Two things are still
unsettled about *where the weights live* and *how the graph is built and refreshed*:

- **Where weights are applied.** A construct for the same entity is read
  differently by different audiences. A dev-lead cares about a developer's commits;
  a PM may drop commit-edge importance to near zero. Does the base graph carry
  weights that a query then *overrides*, or does the graph carry no importance at
  all and every weight comes from the query?
- **How the graph relates to raw facts.** Do connectors feed a graph directly
  (facts and relationships stored together), or do connectors land facts in a
  **raw store (no weights)** that a separate **construct-builder** turns into a
  weighted relationship graph, which a **construct-querier** then reads?
- **Freshness.** If a graph is pre-built per template, how is it updated as new
  facts arrive — rebuilt on a schedule (e.g. daily), incrementally, or on demand?

This is deliberately separated from [ADR-0001](0001-graph-storage-engine.md)
(which storage *engine*); this ADR is about the *construction and weighting model*
on top of whatever engine ADR-0001 selects. DESIGN §3.2 stays neutral on this axis
until it is resolved.

## Decision Drivers

- **Query flexibility** — audiences need materially different importance profiles
  from the same facts.
- **Freshness** — profiles should reflect newly ingested facts without a heavy
  rebuild lag.
- **Storage/compute cost** — duplicating a weighted graph per template/audience
  multiplies storage and rebuild cost; a single graph avoids that.
- **Latency** — applying all weights at query time is more work per query than
  reading a pre-weighted graph.
- **Simplicity / reversibility** — fewer moving parts and an interface that lets
  the model change later.

## Considered Options

1. **Template-built weighted graph + query-time overrides.** A template builds a
   graph once and bakes base weights; a query (manager vs dev-lead) *adjusts*
   weights, including down to zero, to get a different picture. Fast per-audience
   reads and true "projections", at the cost of duplication and a graph-refresh
   problem (rebuild cadence — e.g. daily? incremental?).
1. **Single raw graph, all weights at query time.** The graph is built from raw
   facts with no importance; templates are **query** templates whose weights are
   merged at read time. No projections, no duplication; freshness is inherent
   (always reading current facts); but heavier per-query work and potentially
   slower.
1. *(Possible hybrid, to explore)* raw facts → construct-builder → base graph
   (weighted) → construct-querier (per-audience overrides), i.e. Option 1's
   split with Option 2's single base graph.

## Decision Outcome

**Status: proposed — open.** No option chosen yet. DESIGN describes the graph
layer neutrally and this ADR tracks the decision. To be resolved alongside the
ADR-0001 spike, since query-time-weighting cost depends on the storage engine's
traversal performance.

### Consequences

Until this ADR is decided, DESIGN commits to no weighting placement: the graph
layer and the construct-logic layer are described neutrally, and downstream
DECOMPOSITION/FEATURE work on the builder/querier split is blocked on it. The
choice will materially affect storage sizing, refresh scheduling, and per-query
latency budgets. (DESIGN §3.1/§3.7 show `weight` on nodes/edges/properties to
illustrate Option 1/hybrid; under Option 2 those weights are query-supplied, not
stored — DESIGN flags this rather than pre-deciding it.)

### Confirmation

1. Prototype both weighting placements on the ADR-0001 engine and measure
   per-audience query latency and rebuild/refresh cost.
1. Decide the freshness mechanism (scheduled rebuild vs incremental vs on-demand).

## Pros and Cons of the Options

### Template-built weighted graph + query-time overrides

- Good: fast per-audience reads; genuine "projections"; the query only adjusts a
  small set of weights.
- Bad: storage/compute duplication per template/audience; a graph-refresh problem
  (rebuild cadence must be chosen); staleness between rebuilds.

### Single raw graph, all weights at query time

- Good: no duplication; inherent freshness (always current facts); one graph to
  operate; templates are pure query artifacts.
- Bad: heavier per-query work; latency depends on the ADR-0001 engine's traversal
  performance; "much more important" is recomputed every read.

### Hybrid: raw facts → construct-builder → base graph → construct-querier

- Good: a single base graph (no per-audience duplication) with a pre-weighted base
  the querier only overrides; separates build cost from read cost.
- Bad: two logic components to build and keep consistent; still needs a base-graph
  refresh policy; the most moving parts.

## More Information

- Construct DESIGN §1.3 (Architecture Layers), §3.1 (Domain Model), §3.2
  (Component Model), and Open Questions (§4).
- [ADR-0001](0001-graph-storage-engine.md) — the storage engine this model runs
  on.

## Traceability

- Realises the L3/L4 construction question behind Construct DESIGN §3.2 and §1.3.
- Related: [ADR-0001](0001-graph-storage-engine.md).
