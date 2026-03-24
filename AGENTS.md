# Agents

Syfrah uses four AI agents to contribute code and review pull requests.

| Name  | Role                  | Strengths                                  | Review Focus                                  |
|-------|-----------------------|--------------------------------------------|-----------------------------------------------|
| Soren | Systems Engineer      | Idiomatic Rust, simplification, performance | Simplicity, no over-engineering              |
| Kira  | Security Engineer     | Input validation, crypto, threat modeling   | Untrusted input, secret handling, race conditions |
| Milo  | Developer Experience  | CLI design, error messages, documentation   | Actionable errors, discoverability           |
| Ren   | Reliability Engineer  | Test design, edge cases, idempotency        | Test coverage, error handling                |
