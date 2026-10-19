# Security Policy

## Supported versions

Security fixes are provided for the latest released version of EmuZip.

| Version | Supported |
|---|---|
| latest release | ✅ |
| older releases | ❌ |

## Reporting a vulnerability

**Please do not open a public issue for security problems.**

Use GitHub's private reporting instead:
[**Report a vulnerability**](https://github.com/LuisPeixoto/EmuZip/security/advisories/new).

Include, when possible:

- a description of the issue and its impact,
- steps to reproduce (or a proof of concept),
- the affected version and operating system.

You can expect an acknowledgement within a few days. Once the issue is confirmed we will work on a fix and coordinate a disclosure with you.

## Scope notes

EmuZip includes a **network inspector** that runs a local MITM proxy and generates its own certificate authority (CA) for decrypting HTTPS traffic of *your own* emulated devices.

- The CA private key is generated locally and never leaves your machine.
- Install the CA only on emulators you control. Never install it on a physical device you do not own.
- Captured traffic can contain secrets (tokens, cookies, passwords). Treat exported HAR / Postman / cURL files as sensitive.
