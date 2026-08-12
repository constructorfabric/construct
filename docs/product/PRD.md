# PRD — Construct (Universal Digitwinization Engine)

_**Status:** Draft · **Version:** 0.1_

## Table of Contents

- [1. Overview](#1-overview)
- [2. Actors](#2-actors)
- [3. Goals & Functional Requirements](#3-goals--functional-requirements)
- [4. Operational Concept & Environment](#4-operational-concept--environment)
- [5. Scope](#5-scope)
- [6. Non-Functional Requirements](#6-non-functional-requirements)
- [7. Use Cases](#7-use-cases)
- [8. Dependencies](#8-dependencies)
- [9. Assumptions](#9-assumptions)
- [10. Risks](#10-risks)
- [Open Questions (technical)](#open-questions-technical) — the full register of open and resolved questions is kept separately

## 1. Overview

### 1.1 Purpose

Construct is a **universal digitwinization engine**: it builds and runs living digital twins — Constructs — of any entity: a person, a team, a project, an organization, a product, a process, an AI agent, a system.

An entity qualifies for a Construct when it has a **persistent identity** **(Subject)**, an **evolving state**, an **Owner**, and **consumers** of information about it.

It ships as a **Constructor Fabric Gear** so that any application — Constructor's own or third-party — can create, enrich, and consume Constructs.

Each Construct stays loyal to its Owner, keeps its profile verified and current, and works in three ways:

1. communicates & helps the Owner directly,
1. communicates with Consumers (apps, other people, other people's Constructs) to help them serve the Owner better or to share with them information about its Subject permitted by the Owner,
1. acts on the Owner's behalf.

### 1.2 Background / Problem Statement

Building a profile of anything is a pain of setting up and synchronizing numerous services, connectors, procedures and manual input — slow, error-prone, and stale the moment it is assembled. Enrichment services cover few specific areas of information; account profiles and CRMs go stale; existing digital-twin platforms model machines and buildings, not something that is constantly changes; every tool builds its own siloed picture.

This PRD defines Construct — the engine any application can use to provide such a memory-and-profile layer, built on common Fabric technology (GTS, connectors, Evidence Graph) rather than a private stack.

**Target users — two kinds:**

*Builders — those who take the engine (Fabric / Gear adopters) to build Constructs into their software:*

- Application teams that embed the Gear so their product gets Constructs of its users or domain objects
- Organizations that want the model of their domain (people, teams, processes, products) collected once, kept current, and reusable across their tools

*End users — those who use the built Constructs:*

- Owners — a person for their own Construct; a governing role for an entity's (a CEO or board for an organization, a department head, a project sponsor) — running living, loyal profiles of themselves or their entities
- People and teams served through Construct-powered applications — they benefit from personalization without operating anything

_(Subjects who are not Owners are affected parties with rights — consent, review, deletion where the law grants them — not target users; see §2 Actors.)_

**Key problems solved:**

- No single engine that collects, verifies, stores, and serves a living profile of an arbitrary entity — with permissions, provenance, and loyalty built in.

### 1.3 Glossary

| Term                       | Definition                                                                                                                                                                                                                                                                                    |
| -------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Construct (engine)         | This product — the universal digitwinization engine described by this PRD, shipped as a Fabric Gear                                                                                                                                                                                           |
| a Construct                | A living digital twin of one Subject, built and run by the engine                                                                                                                                                                                                                             |
| Owner                      | Who creates and manages a Construct and whose interests it serves. Always a person or a group of people in a governing role: the person themself for a self-Construct; a CEO or board for an organization's; a department head, a project sponsor                                             |
| Builder                    | Whoever defines what Constructs of a type know and how, at build time: our product teams, a host application's team, an organization's internal team, or a community Contributor extending the open core. Owners define schemas too — for their own Constructs, conversationally, at run time |
| Subject                    | The person or entity a Construct is about; may coincide with the Owner                                                                                                                                                                                                                        |
| Curated profile            | The Construct's data: source-linked facts, confidence-scored insights, Owner-announced goals and preferences                                                                                                                                                                                  |
| Situation tag              | The context in which a fact or preference applies (role, setting); prevents one-bucket profiles                                                                                                                                                                                               |
| GTS (Global Type System)   | Fabric's schema registry: vendor/package/namespace/type/version with validation rules; the "data contract police" between collectors and interpreters                                                                                                                                         |
| Connector                  | A collector that brings data from one source system and declares its schema; written once, reused everywhere                                                                                                                                                                                  |
| Evidence Graph             | The scalable weighted-graph store (nodes, edges, properties) all collectors write into, validated against GTS contracts; holds raw artifacts and derived intermediates, all with lineage                                                                                                      |
| Derivation record          | For any derived fact or insight: the list of evidence it rests on plus the algorithm or model that produced it, with version and provider (engine, host application, or Insight)                                                                                                              |
| Construct Network          | The links between Constructs, with per-context views (work vs social); basis for peer discovery, team assembly, comparison                                                                                                                                                                    |
| Connection protocol        | The open specification for how services, homes, and hosts talk to Constructs: operations API, permission-slice and data-agreement formats, export/import. The boundary between this PRD (engine) and the platform                                                                             |
| Construct engine (layer 4) | The workflow layer that traverses the Evidence Graph by weights, summarizes, and produces a Construct version at a chosen cost/quality point                                                                                                                                                  |
| Construct version          | Schema + algorithm; multiple versions run in parallel (v1 and v4 can both be live)                                                                                                                                                                                                            |
| Template                   | A ready starting kit for a known role or entity type: schema + weights + connector set                                                                                                                                                                                                        |
| Open core model            | The open-source model of an entity type: its schema plus its foundational insight model (for a person — the Mastery-level model). Contributors extend it; applications build and sell their own extensions on top                                                                             |
| Minimal live engine        | The open-source runtime an Owner takes with an extracted Construct so it never degrades into a dead data dump: local store, schema runtime, Owner-directed collection, the entity type's foundational insight model, refresh, chat, and the serving API with permissions (§4.1)               |
| Construct home             | A future single place where an Owner connects every Construct they want to keep alive                                                                                                                                                                                                         |
| Consumer                   | Any application, AI assistant, or other Construct that requests a Subject's context                                                                                                                                                                                                           |
| Insight                    | Constructor Insight — the analytics product; computes insights about any entity (a person, a team, a process, the organization) and files durable ones into the corresponding Constructs                                                                                                      |
| Telemetry layer            | The Proctor telemetry family (FE SDK, BE SDK, CM desktop) — the sensing layer Construct consumes activity through                                                                                                                                                                             |

## 2. Actors

### 2.1 Actors

#### Owner

**ID**: `cpt-cst-actor-owner`
**Role**: Creates and manages Constructs; announces goals; grants permissions; may extract the Construct. Always a person or a group of people in a governing role — the person themself, a CEO or board for an organization, a department head, a project sponsor. A company as such cannot be an Owner; its governing people are.
**Needs**: Loyalty (the Construct serves them alone), control over disclosure, portability.

#### Subject

**ID**: `cpt-cst-actor-subject`
**Role**: The entity a Construct is about. For self-Constructs, same as the Owner; for memory-Constructs and client-Constructs, a different party.
**Needs**: Legal rights honored (view/correct/delete where applicable), no leaking beyond permissions, verified facts changeable only at their source.

#### Builder

**ID**: `cpt-cst-actor-builder`
**Role**: Defines what Constructs of a type know and how they learn it: schemas with their full kit (sources, verification, validation), templates, and versions (cap. 16). In practice: our product teams, a host application's team, or an organization's internal team; a Contributor is a Builder working in the open. An Owner takes this role for their own Constructs through the conversational path (cap. 3).
**Needs**: Ready templates to start from; AI-assisted schema authoring; strict validation feedback from the engine; a stable GTS and clear documentation.

#### Host App Integrator

**ID**: `cpt-cst-actor-integrator`
**Role**: Embeds the Gear, maps app data to GTS contracts, and wires the app's features to the serving API. The Integrator consumes no context themself — the host app's features do, on behalf of its users and their Constructs (see Consumer, §2.2).
**Needs**: One stable integration path; templates; clear data contracts.

#### Template / Model Contributor

**ID**: `cpt-cst-actor-contributor`
**Role**: Extends the open core model with new entity types, templates, connectors — inside the company or in the open-source community.
**Needs**: Schema evolution without breaking guarantees; a place to publish (future marketplace).

#### Data Protection Officer

**ID**: `cpt-cst-actor-privacy`
**Role**: Governs consent, data classes, retention, and regional regulation across all collected data.
**Needs**: Access log, provenance on every item, deletion notices reaching every data holder (cap. 20), per-source bans, auditable permission model.

### 2.2 System Actors

#### Connectors

**ID**: `cpt-cst-actor-connector` — bring data from source systems (HR, M365, GitHub, social, contract stores…), each declaring its GTS schema; shared with Insight where possible.

#### Telemetry layer (Proctor family)

**ID**: `cpt-cst-actor-telemetry` — supplies in-app and on-device activity signals and person-relevant derived facts; the only path for behavioral data — host apps do not build their own activity collectors.

#### Constructor Insight

**ID**: `cpt-cst-actor-insight` — computes insights about any entity (people, teams, processes, the organization) over its evidence model and writes durable conclusions, with lineage, into the corresponding Constructs.

#### Host / Consumer Applications

**ID**: `cpt-cst-actor-consumer` — request context slices and serving guidance; act in their own domain (pedagogy, assessment, meetings); never receive more than permissions allow.

#### Fabric runtime (future)

**ID**: `cpt-cst-actor-runtime` — serverless workflow runtime considered for construct-engine orchestration (resumable workflows, concurrency caps per Subject).

## 3. Goals & Functional Requirements

### 3.1 Goals (Business Outcomes)

**First:**

1. Ship person-Construct as a Fabric Gear: built on the shared Fabric architecture (GTS + connectors + Evidence Graph) inside, one integration path for any host application outside.
1. Launch the person-Construct pilot to production in Constructor Tech (Learn & Groups scenario) — running on this Gear, not on the old components.

**Second:**

1. Ship Constructs of non-person entities (team, project, organization, process) on the same engine.
1. Launch person- and non-person- Constructs in Studio.
1. Extend the list of Construct scenarios in Constructor Tech.

**Third:**

1. Launch Construct as autonomous actor in Studio.
1. Enable extraction: the Owner takes the living Construct out (minimal live engine).

### 3.2 Capabilities (functional requirements)

_Grouped in lifecycle order: create a Construct → work with one Construct → govern its data → connect Constructs to each other. Every requirement names its actor — a role in bold (Builder, Owner, Subject, Consumer), or **the engine** when it happens automatically._

**A. Creating a Construct**

1. **The engine ships with** ready templates for Constructs of various types (person, team, process, organization, etc.) and for various domains (e.g. person-Construct for educational domain - learner, instructor; for software development domain - product manager, developer, QA, etc.): a versioned schema where every attribute carries its collection routes (which sources, which connectors), verification rules, validation rules, and links to other entity types (a person belongs to a team; a product competes with other products) working out of the box.

1. **A Builder** adds their own templates for other domains, supplying the same full kit: attributes, sources and connectors, data contracts, verification rules.

1. **A Builder**\* defines what a Construct of a given type knows and how it learns it — by taking a ready template (cap. 1-2) and/or by defining additional pieces for the templates. **An Owner** can do the same for their own Constructs, without waiting for a Builder — conversationally: ask your Construct to create a Construct of an entity and tell it what data you want collected; **the engine**, AI-assisted, turns the request into a proper schema with the full kit and validates it the same way (cap. 4).

1. **The engine** validates every added piece or template and guarantees extensions never break existing data.

1. **A Builder's application** starts a Construct by a defined signal from a seed: name, email and role for a person; a charter for a project; a URL for a product. \*\*The engine takes it from there.

1. **The engine** collects from every permitted source:

   - organizational systems — via connectors;
   - the public internet — with layered search;
   - activity and behavior — only through the telemetry layer: one sensor, one consent gate, no per-app collectors;
   - domain data held by host applications (grades, submissions, feedback, existing user context) — via connectors under GTS contracts;
   - chats with in-app AI assistants;
   - sources the Owner connects, and facts and goals the Owner states directly;
   - durable insights computed by Insight.

1. **The engine** collects in 2 modes: directed — from sources the Owner pointed at, and proactive — from sources the engine found itself.

1. **The engine** verifies facts, scores them with confidence, and stamps every item with its full origin.

1. **The engine** resolves identity for any Subject: before attaching new evidence or creating a new Construct, it decides whether the data, the source, or an existing Construct refers to the same Subject — one entity, one identity, no accidental duplicates and no mixed-up namesakes.

1. **The engine** marks every fact and preference with the situation it applies to (role, context) — a preference is not global.

1. **The engine** stores everything it collects and everything it derives in the Evidence Graph: the items, the links between them, and their attributes — each carrying its source, and a weight that says how much it counts when conclusions about the Subject are drawn.

1. **The engine** keeps every Construct current on its own: it re-collects from its sources and refreshes facts, insights, and every field of the schema without being asked — staying up to date is a Construct's default state, not a task.

**B. Working with one Construct**

13. **Any permitted actor** — the Owner always, Consumers within their permissions — invokes the standard operations on a Construct: read, write, share context, anonymize, transfer, extract, compare — plus delegate and notify (subscribe to changes).
01. **The engine** serves any allowed Consumer — an app, an AI assistant, another Construct — a permission-filtered slice of context, plus guidance on how to serve this Subject well.
01. **The owner** can add any missing attributes - in this case, engine creates an additional Construct version - a view - for this owner.
01. **The engine** runs any number of versions side by side over the same evidence. Common fields update once, for all versions at a time; fields unique to one Owner update only in that Owner's version.
01. **The Owner** extracts the Construct with the open-source minimal live engine: it keeps collecting, updating and serving on Owner's side, with the entity type's foundational insight model (see §4.1).

**C. Data governance and permissions**

18. **The engine** enforces the security model **the Owner** configures: per-consumer permissions by data category, ready presets, and a full access log — who took what, and when.
01. **The subject** can ask their own Construct — or the Owner of any Construct holding their personal data — to show, fix, or delete it, and the outcome might be: delete, anonymize, or a recorded lawful refusal (depending on the applied law). An organization's or a team's data is governed by contracts — NDAs, employment and partner agreements: what may be stored, who may see it, and what may leave with a departing person. Deletion reaches derived conclusions too (traceable via cap. 8), with the impact shown to the Owner before it runs.
01. **The engine** keeps watch after data leaves (the watchguard function):
    1. Every shared piece is marked, so together with the access log (cap. 18) **the engine** always knows who holds what.
    1. When the Owner deletes or revokes, **the engine** notifies every holder and tracks who confirmed.
    1. **The engine** alerts the Owner about suspected leaks or misuse, ignored notices, and data whose agreed retention is over.

**D. Between Constructs**

21. **The engine** maintains the Construct Network: links between Constructs, with per-context views — a work network and a social network overlap but do not coincide. The basis for peer discovery, team assembly, and comparison.
01. **Any Consumer with permission** compares Constructs of the same type and finds look-alikes by defined list of attributes.
01. **The engine** recognizes from the Owner's ongoing work that a Construct is needed and, per the Owner's settings, creates it or suggests creating it. Before creating, it checks whether the organization already holds a Construct of the same Subject and if yes, instead of duplication requests access for the Owner.
01. **The Owner** asks their Construct to monitor for new Subjects within a Construct type. **The engine** adds new Constructs for newly identified Subjects.

## 4. Operational Concept & Environment

The engine is four systems, by deliberate analogy with a file system (data structures + operations + access rights + a directory graph):

1. **Type system (GTS)** — taken ready from Gears. Describes every entity type and data contract: vendor, package, namespace, type, version; validation rules; optional/required. One description serves Construct, Insight, and Gears.
1. **Connectors** — many, each written once. Each brings its schema; the same connector feeds Construct and Insight. Insight's existing connector fleet is the starting point.
1. **Evidence Graph** — scalable weighted graph (nodes, edges, properties; properties carry per-source attribution) holding raw artifacts and derived intermediates with lineage. Validates on insert against GTS contracts ("ты сам свой контракт нарушил — ошибка"). Does not exist yet as a graph API — Insight has bronze/silver/gold raw layers; the Studio team carries the strongest graph expertise.
1. **Construct engine** — turns evidence into Constructs: walks the Evidence Graph starting from what weighs most, summarizes what it finds, and fills the schema — quickly and cheaply or deeply and thoroughly, as chosen. Several versions of a Construct can be built this way from the same data at once.

### 4.1 Minimal live engine composition

In plain words: when an Owner leaves with their Construct, they take a small free program plus their data — not a dead folder of files. It keeps the Construct alive on the Owner's own machine.

Must have (v0 — defines the open-source surface):

1. **Local store** — the Construct plus its slice of the Evidence Graph, embedded, no cloud dependency.
1. **Schema runtime** — a read-only GTS snapshot with validation, so data stays typed and extensible.
1. **Collection loop** — Owner-owned sources only (device activity, calendar, mail, Owner-announced facts, LLM with owner subscription or local model - bring-your-own-model: keeping it alive costs us nothing).
1. **Foundational insight model of the entity type** — e.g. for a person Construct, its' the Mastery-level model: it keeps observing and assessing from local evidence. This is what makes the extracted Construct useful, not merely alive — and it means the foundational model core ships open source.
1. **Refresh** — periodic recompute of the profile and the foundational model at the good-enough cost tier, using the Owner's own LLM key or a local model.
1. **Chat with your Construct** — an LLM connection so the Owner can interact with their data: ask, explore, correct.
1. **Serving API with permissions** — the standard operations served locally, permission-filtered, with the access log.

Should have (v1):

1. Watchguard basics (the disclosure ledger travels along, alerts continue),
1. The reconnect protocol (plug into a new host platform or analytics engine; receive schema updates).

Explicitly not in minimal:

1. host-app premium features,
1. proactive enrichment services,
1. Insight analytics,
1. community templates.

Several Constructs under one minimal engine is the seed of the future Construct home — the same product at n=1 and n=many.

Environment constraints:

- Ships as an open-source Fabric Gear; the open **core** model is reusable by anyone, host applications sell their own extensions on top.
- Person-Constructs operate under GDPR-class regulation: consent, right to view/correct/delete with deletion notices to every recipient (cap. 20), per-source bans; on-device processing where media is involved (telemetry layer's invariant: the flag leaves the device, the picture never does).

## 5. Scope

### 5.1 In Scope

- The four-layer engine: GTS adoption, connector fleet (reused + new), Evidence Graph storage, construct-engine workflows.
- Person Construct as the flagship: creation from seed, six-source enrichment (org systems, public internet with layered search, in-app activity via telemetry, Owner-connected sources, Owner announcements, Insight-fed insights).
- Curated profile with provenance, confidence, and situation tags.
- The standard operation set (§3.2 cap. 13) and the security model with access log.
- Serving: permission-filtered context slices + serving guidance/recommendations to consumers.
- Construct versions and domain-specific templates (learner, instructor, developer, QA as first set).
- Subject rights implementation (view/correct/delete, deletion notices via the watchguard, transfer filter, source bans).
- Extraction with the minimal live engine (open source).
- Integration as a Gear: Constructor apps first (Learn pilot), third-party hosts by design.

### 5.2 Out of Scope

- Signal capture (the telemetry layer / Proctor family owns sensing; this PRD consumes its output).
- Org-level analytics computation (Insight computes; Construct stores durable conclusions).
- Host-app personalization and pedagogy decisions (consumers act; Construct advises with context — router, not generator).
- Dashboards and presentation UI (host side).
- The universal Construct home, service marketplace, and ecosystem operations.
- Marketplace for community templates — platform side, same companion doc.
- Raw-media processing of any kind.

## 6. Non-Functional Requirements

- Graph scale envelope: on the order of 100M objects / 500M links per large tenant — storage choice must not preclude it. `[verify]`
- Construct computation must have controllable cost tiers (good-enough vs. deep).
- Context serving to consumers: near-real-time (Construct today serves near-real-time; exact latency target TBD).

## 7. Use Cases

_Scenarios live here; the capabilities they exercise (§3.2) are cross-referenced in brackets. Caps 4 and 6–12 are internal machinery — every scenario exercises them implicitly._

- **Learn (pilot)** — personalized learning via person Constructs: skip the known, explain at the learner's level, mastery tracking. [caps 5, 6, 8, 10, 14]
- **Groups / Assess / Proctor** — session summaries, level-fitted feedback, exam-anxiety assistant consuming Construct context. [caps 6, 14, 18]
- **Corporate / Studio** — personal preferences extracted from communication history and applied to message drafts; team capability maps; the autonomous-actor scenario (assistant → semi-autonomous → autonomous); find the person or team with a unique skill through Constructs. [caps 6, 14, 21, 22]
- **Competitive comparison & market watch** — a PM starts comparing their product with competitors. Their Construct notices the task and suggests (or creates, per settings) a Construct for each product in the comparison [cap 23]; the PM adjusts what is collected conversationally [cap 3]. Where the organization already holds a competitor's Construct, the PM requests access from its Owner instead of duplicating; attributes the PM lacks are added by that Owner's approval or as the PM's own version, with common fields updating everywhere at once [caps 23, 15, 16]. The PM compares the set [cap 22]. To watch the market, the PM asks Construct to monitor for any new competitors; the engine keeps it current, and each new competitor arrives as a fresh Construct with a notification [caps 24, 12, 23].
- **Candidate/partner profiles** — candidate-owned Constructs, CV per vacancy; partner passports. [caps 13, 14, 17, 19]
- **Adopting the engine in a new domain (Builder)** — an organization takes the Gear for a domain we don't cover (say, legal work). It starts from the nearest shipped template or builds its own [caps 1, 2], defines the schema with collection routes and verification rules for each attribute [cap 3], connects its sources, and seeds the first Constructs [cap 5]; the engine collects and verifies from there, and keeps it all current [caps 6, 8, 12].
- **A departing person takes their Constructs** — an employee leaves and extracts their self-Construct and their memory-Constructs of colleagues with the minimal live engine [cap 17]. The transfer filter decides what travels: "my colleague JD is from Portugal" goes along — the Owner's own knowledge of a person; "'s salary is €85k," known to them as JD's manager, stays behind — the company's confidential data, no matter whose Construct it landed in [cap 19]. The watchguard ledger travels too: past disclosures stay tracked and revocable [cap 20].
- **Process transfer & benchmarking** — a process Construct as portable best practice between organizations; organizations contribute processes anonymized, Insight benchmarks them, above-benchmark processes become transferable templates (the Construct↔Insight link named on Aug 10). [caps 13, 16, 17]

## 8. Dependencies

| Dependency                                                                                                 | Description                                                                                                                                                        | Criticality |
| ---------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------- |
| GTS (Global Type System, Gears)                                                                            | Ready type-system taken from Fabric; schema registry for all contracts                                                                                             | p1          |
| Evidence Graph storage                                                                                     | Does not exist yet as a graph API; build vs. lean on Insight's plans is open; Studio team has the expertise (Vasil Shandyba, Andrey Kuchma — GraphDB demo pending) | p1          |
| Insight connector fleet                                                                                    | Reuse for organizational sources; connectors may become a shared gear                                                                                              | p1          |
| Telemetry layer (Proctor FE/BE SDK, CM desktop; mobile collection in progress at Knowledge Lab `[verify]`) | Sole path for in-app and on-device activity into Constructs; desktop + mobile together make the person Construct extractable                                       | p1          |
| Compliance & security review                                                                               | Gates the first production tenant                                                                                                                                  | p1          |
| Fabric serverless runtime                                                                                  | Candidate orchestrator for construct-engine workflows (resumability, concurrency caps)                                                                             | p2          |
| Host applications (Learn first)                                                                            | Embed the Gear and consume context                                                                                                                                 | p1          |

## 9. Assumptions

- GTS is production-ready and genuinely reusable for Construct's contracts.
- Insight and Construct can share connectors and, eventually, the Evidence Graph storage without merging products (memory vs. analysis split holds).
- Subjects will opt in when loyalty and portability are real (the killer-model bet).
- The open-core commercial model (core open, host extensions sellable) gets SB sign-off.

## 10. Risks

- Cost blow-up: deep Construct computation over large graphs is expensive; without cost tiers the unit economics fail.
- Users decline consent; the engine must stay useful on minimal data.

______________________________________________________________________

## Open Questions

_Major technical questions live here._

- [ ] **Evidence Graph storage: build, buy, or lean on Insight/Studio?** Engine choice constrains query complexity and performance.
- [ ] **Structure of a served Construct** — hybrid document (tree with mandatory per-attribute properties: category, source, situation) — design doc needed.
- [ ] **Workflow orchestration** — plain API workers now, Fabric serverless runtime later?
- [ ] **Final operation set** — write, delegate, notify(subscribe) raised on Aug 11; anything else?
- [ ] **Situation-tagging model** for facts and preferences — known unresolved problem.
- [ ] **Connector split** — which come from Insight as-is, which are written new, and does a shared "extractor" gear own them?
- [ ] **Subject identity resolution** — how cap. 9 is implemented: matching signals, thresholds, disambiguation of namesakes — so reuse-before-create (cap. 23) works across the organization.
