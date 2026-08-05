# 実装状況

最終更新: 2026-08-05

Phase 0〜18はv1.0.0で完了した。v2.0.0ではprovider versionの権威を
compile-time pinから署名付きCompatibility Catalogへ移行した。

## v2 Catalog migration

| 項目 | 状況 |
| --- | --- |
| Catalog domain/schema/bootstrap/signature/cache | complete |
| ProviderDriver identity/artifact probeとTOCTOU検査 | complete |
| deny-first resolverとimmutable provider lock | complete |
| config schema v2と明示migration | complete |
| SQLite schema v2、Rust API、IPC v2 | complete |
| compatibility/provider/config CLIとdoctor | complete |
| qualification evidence、candidate/qualification/publish workflow | complete |
| generated compatibility docs、packaging、2.0 release gate | complete |

詳細な契約と検証項目は
[`provider-version-catalog-migration-plan.md`](provider-version-catalog-migration-plan.md)と
[`V2_DEFINITION_OF_DONE.md`](../V2_DEFINITION_OF_DONE.md)を参照する。

## v1 implementation record

| Phase | 状況 | 既存資産 | 主な不足 |
| ---: | --- | --- | --- |
| 0 | complete | 4-provider scope、契約、非目標、ADR、compatibility、DoD | なし |
| 1 | complete | closed ID、UUIDv7 handle、5-state validator、snapshot、admission/failure分離、exhaustive property test | なし |
| 2 | complete | internal fake ACP、registry、nonblocking actor、event bus、global sequence、failure/permission/flood/cancel/session-change fixture | なし |
| 3 | complete | 公式ACP SDK、version negotiation、authenticate、session/prompt/update/cancel、permission、root制限filesystem、owned terminal host、bounded output、negotiated capability snapshot | なし |
| 4 | complete | self-supervisor、watchdog pipe、専用process group、全exit pathのTERM/KILL/reap、stdout drain、child/grandchild test、escape検出fixture、反復lifecycle stress | なし |
| 5 | complete | Grok実行経路、exact version probe、native ACP command、認証済み実CLI prompt成功、未認証実CLIの`authentication_failed`、supervisor環境allowlist、crash/startup failure分類、process cleanup、段階別metrics | なし |
| 6 | complete | bounded output、receipt、event-driven `wait_run`、公開event stream、global sequence、tool/reasoning projection、lag diagnostic、slow-consumer stress | なし |
| 7 | complete | persistent process/session、bounded mailbox、send/followup、session stamp、cutoff、strict continuity、configurable idle TTL | なし |
| 8 | complete | ACP cancel、queued/running interrupt、deadline統合、grace、force escalation、idempotency、continuity判定 | なし |
| 9 | complete | strict FIFO scheduler、global/provider別Run上限、completion sequence、event-driven `wait_any/all`、入力順保持、100-Agent stress | なし |
| 10 | complete | `agentmux serve`、0600 UDS、versioned bounded IPC、全Control/Wait/Watch CLI、共有Agent、stale socket回復、shutdown barrier | なし |
| 11 | complete | authenticated suite、4 manifest別controlled fault、実provider parent-death cleanup、pinned capability inventoryを固定。broker非対応optional機能も型付きsnapshotで明示 | なし |
| 12 | complete | Cursor Agent `2026.07.20-8cc9c0b` manifest、auth、stream/followup/output boundary/cancel recovery/shutdown、fault/process/capability audit合格 | なし |
| 13 | complete | Codex ACP `1.1.9` / bundled Codex `0.145.0` manifest、auth、stream/followup/output boundary/cancel recovery/shutdown、controlled fault、実process audit、実approval allow/deny、Codex metadata projection合格。`read-only` + user reviewerをtested manifestで固定 | なし |
| 14 | complete | Claude ACP `0.64.2` / Agent SDK `0.3.220` manifest、auth、stream/followup/output boundary/cancel recovery/shutdown、controlled fault、実process audit、実permission allow/deny、permission/terminal/nested-agent metadata envelope合格 | なし |
| 15 | complete | metadata-only SQLite、redacted terminal receipt、restart reconciliation、旧Agent continuity loss、v0→v1 transactional migration、未来schema/破損DB拒否、daemon標準DB | なし |
| 16 | complete | 0600 strict profile parser、checksum/version pin、supervisor境界のenvironment allowlist、default-deny permission、secret redaction、bounded stderr/output/IPC、symlink escape拒否、`doctor --json`（version/auth/socket/SQLite/profile） | なし |
| 17 | complete | Mac Studio release budget、fake 10,000 Run、real 1,000 process、race、stderr/auth/permission/SQLite/socket/process chaosが合格 | なし |
| 18 | complete | v1.0.0、決定的archive、checksum、SPDX、third-party license、compatibility JSON、Homebrew formula、GitHub/Sigstore署名済みartifact、全利用文書 | なし |

## v1以後の変更

provider pin、公開契約、SQLite schemaまたはprocess ownershipを変更する場合は、
対応するconformance、互換manifest、ADR、release gateを同じ変更で更新する。
