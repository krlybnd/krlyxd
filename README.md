# krly3d

Exact B-rep geometry kernel in Rust that an AI agent can model with: every answer is structured data
plus a short text, every face and edge has a stable name, and every capability is a plugin.

The kernel is being rebuilt from scratch on one plugin pattern.

* [SPEC.md](SPEC.md) – architecture, design patterns, families, the AI-facing half, phases
* [HANDOFF.md](HANDOFF.md) – what exists today, how to build and test, what was verified
* [docs/architecture.drawio](docs/architecture.drawio) – the architecture diagrams, editable

```bash
cargo test --workspace
```
