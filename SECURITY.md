# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.4.x   | ✅ latest only |
| < 0.4.0 | ❌ (pre-public development versions) |

Only the latest released minor line receives fixes. If you run an older version, upgrade.

## Reporting a Vulnerability

Please report privately via GitHub's private vulnerability reporting:

**https://github.com/CallMeHFK/router-mcp/security/advisories/new**

- You will get a human acknowledgement within **3 working days**.
- Fixes are developed on a private advisory fork and disclosed together with the fix release. Reporters are credited unless they prefer not to be.
- Please do not open public issues or discuss suspected vulnerabilities publicly before the coordinated disclosure.

### Scope notes

This project runs **as root on the user's router**. Anything that would let an unauthenticated caller invoke a tool, bypass the HTTP bearer token, escape the whitelisted command surface (shell injection, path traversal through tool arguments), or corrupt router configuration files is in scope. LAN-adjacent risks (plaintext HTTP transport on a trusted LAN) are documented design trade-offs — still report actual bypasses of the token check or the fail-closed startup.
