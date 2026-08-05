# 新計画に対する現状実装差分

最終更新: 2026-08-05

この表は実装とrelease actionを分けたgap記録である。各PhaseのExit Gateをすべて
満たすまで`complete`にはしない。

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
| 18 | partial | v1.0.0 version、決定的archive、checksum、SPDX、third-party license、compatibility JSON、Homebrew formula、GitHub/Sigstore attestation workflow、全利用文書 | `v1.0.0` tagをpushして署名済みartifactを公開する外部release action |

## 次の変更単位

1. 全local gateとrelease archiveの再現性を再検証する。
2. maintainerが`v1.0.0` tagをpushし、署名済みartifactを公開する。

依存順を飛び越えて実装済みの機能があっても、その前提PhaseのExit Gateが閉じるまで
stableとは扱わない。
