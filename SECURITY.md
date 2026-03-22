# Security Policy

## Reporting a vulnerability

If you discover a security vulnerability in Syfrah, please report it privately.

**Email:** security@syfrah.dev

Do **not** open a public GitHub issue for security vulnerabilities.

## What to include

- Description of the vulnerability
- Steps to reproduce
- Affected versions/components
- Impact assessment (if possible)

## What to expect

- Acknowledgment within 48 hours
- Status update within 7 days
- Fix timeline communicated once the issue is assessed

## Scope

This policy covers the Syfrah codebase and its dependencies. It does not cover infrastructure operated by third parties (hosting providers, S3 services).

## Known security characteristics

Syfrah's security model is documented in [layers/fabric/README.md](layers/fabric/README.md#security-model). Key points:

- The TCP peering channel (join ceremony) is **not TLS-encrypted**. The mesh secret is transmitted in plaintext over TCP during join. The WireGuard mesh provides encryption after joining.
- The mesh secret is the root of trust. Compromise of the secret allows joining the mesh and decrypting peer announcements.
- PIN auto-accept uses a 4-digit code. It is designed for convenience during bootstrap, not as a long-term security mechanism.
