# 実装状況

Status: v2.0.0 runtime-compatibility migration complete（2026-08-05）

実装済み:

- Codex / Claude / Grok / Cursorの閉じた4 providerとstable ACP v1
- Handle-first control API、event stream、pollingなしのwait API
- 5状態Run machine、Agent actor、mailbox、strict same-session followup
- cancelからowned process group回収までのinterrupt/deadline経路
- best-effort version/component観測、optional exact local assertions
- executable owner/mode/type/hash検査とspawn直前TOCTOU再検査
- ACP initialize/capability gateと`ProviderExecutionIdentity`のsnapshot/receipt伝播
- final config schema v2、IPC v2、SQLite schema v3と旧データ移行
- `provider inspect`、`config migrate`、runtime-oriented `doctor`
- UDS broker、metadata-only persistence、security bounds/redaction
- fault/race/chaos/soak/benchmark/reproducible packaging suites

未知version、非semver、probe failureはassertionがなければ標準経路でACP initializeへ進む。
provider release追跡は製品責務ではなく、driver contractが破壊された場合だけコード更新を行う。
実provider auditは認証情報とquotaを使うためmanual/ignored suiteとしてrelease時に実行する。
