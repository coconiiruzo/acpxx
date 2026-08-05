# Provider qualification evidence

Evidence files record authenticated provider qualification inputs and results.
Published Catalog entries contain the SHA-256 of the reviewed evidence bytes.
Discovery never publishes an entry automatically; maintainer approval and the
protected Catalog-signing workflow are separate steps.

Evidence contains provider identity and artifact digests, not credentials,
tokens, prompts, model output, or environment values.

Schema-v1 qualification evidence records target/OS, agentmux commit and binary
digest, Driver ID/revision, suite version, start/end time, exact provider
identity/artifacts, initialize/auth/session creation, stream ordering and output
boundaries, at least three same-session turns, cancel recovery, permission
deny/allow behavior, crash/malformed-protocol classification, process cleanup,
capability inventory, final result, and a reviewer approval reference.

`provider-candidate-discovery.yml` may only open an issue.
`provider-qualification.yml` runs explicitly on the protected authenticated
macOS arm64 runner and uses `record-provider-qualification.py` to create new
evidence. `compatibility-publish.yml` requires protected approval and signing
key access; it publishes a complete higher-sequence Catalog rather than a
partial patch. Corrective action is another higher sequence.
