# Security Policy

This project is pre-release. Do not use it as a security boundary for untrusted
providers or prompts.

Please report vulnerabilities privately through
[GitHub Security Advisories](https://github.com/coconiiruzo/acpxx/security/advisories/new).
Do not open a public issue containing credentials, exploit details, or sensitive
prompt/output data.

`acpxx` does not intentionally persist prompts or outputs. Permission requests
are denied by default. The Phase-1 process owner provides Unix process-group
cleanup during normal broker operation, but parent-death supervision and Windows
Job Object containment are not implemented yet.
