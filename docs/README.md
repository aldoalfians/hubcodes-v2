# hubcodes documentation

| Document | Audience | Content |
|---|---|---|
| [architecture.md](architecture.md) | engineers | components, module map, data model, sync loop, message path, phase-2 daemon design, diagrams |
| [ui-ux.md](ui-ux.md) | design, engineers | canvas principles, node anatomy, edge routing rules, colour, interaction modes, known UX issues |
| [providers-and-protocol.md](providers-and-protocol.md) | engineers, agent authors | provider adapter spec, status heuristics, message protocol v0, CLI reference, prompt snippet for agents |
| [sharing-and-security.md](sharing-and-security.md) | engineers, security | sharing topologies over Tailscale, permission model, threat model, audit |
| [reference-teardown.md](reference-teardown.md) | product, design | evidence-based teardown of Maestri and T3 Code, capability map |
| [roadmap.md](roadmap.md) | everyone | phases, scope, acceptance criteria |
| [evidence/README.md](evidence/README.md) | reviewers | captured evidence (REA), hashes, how it was produced |
| [screenshots/](screenshots/) | everyone | PNG renders of the spike |

Conventions used in these documents:

- **Observed** means verified by running the code or by a captured artifact in `docs/evidence/`.
- **Designed** means specified here but not implemented.
- **Assumed** means a working assumption that must be checked before building on it.

All diagrams are Mermaid and render on GitHub.
