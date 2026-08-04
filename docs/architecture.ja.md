# 統一コーディングエージェント・ランタイム計画

## 結論

`acpxx` は、Codex・Claude・Grok・Cursor・Antigravityを同じHandle-first
APIで操作するローカルAgent Brokerを目指す。公開APIは論理的な
`AgentHandle` / `RunHandle`だけを参照し、内部transportはACPに固定する。

初回変更の範囲はPhase 0のdomain contractと、Grokだけを利用するPhase 1の
one-shot vertical sliceに限定する。continuity、interrupt、複数provider、
daemon、永続化は、このsliceのexit gateを通過するまで実装しない。

## 公開操作

| 操作 | 対象 | Run生成 | 意味 |
| --- | --- | ---: | --- |
| `spawn` | `ProviderSpec` | する | Agentと初回Runを受理し、非同期でproviderを起動する |
| `send` | `AgentHandle` | しない | runtime mailboxへ追加し、実行を開始しない |
| `followup` | 前回の`RunHandle` | する | 同一ACP sessionで次のpromptを開始する |
| `interrupt` | `RunHandle` | しない | 対象Runだけを中断する |
| `list` | filter | しない | Agent、Run、provider expectationを返す |
| `wait_run` | `RunHandle` | しない | terminal receiptを待つ |
| `wait_any/all` | Run集合 | しない | terminal receiptを集約する |

Phase 1では`spawn`、`list`、`wait_run`だけが実行可能である。その他の
signatureは固定済みだが、呼び出すと明示的な`NotImplemented`を返す。

## Identityと状態

`AgentId`、`RunId`、`MessageId`はUUIDv7とする。provider名、cwd、path、
display name、tree pathを識別子にしてはならない。

Runのcanonical stateは次の5つだけである。

```text
queued -> running -> succeeded
                  -> failed
                  -> interrupted
```

`starting`、`cancelling`、`timed_out`、`continuity_lost`等はstateに追加せず、
`stage`、`stop_reason`、`failure.code`、`continuity`で表現する。Agentには
別のstatus enumを設けず、process、continuity、active/latest Run、mailbox
depthを直交フィールドとして保持する。

## Runtime

```text
Broker API
  +-- Registry (Agent / Run)
  +-- watch-based Wait Hub
  +-- global Semaphore
       `-- Agent actor
            `-- official ACP Rust SDK
                 `-- provider process
```

Agent actorがRun transitionとprovider eventの順序を所有する。同じAgentで
同時に実行できるRunは1つだけとする。異なるAgentの並列度はglobal
semaphoreで制限する。

各Runは`tokio::sync::watch` channelを持つ。subscriber作成後に現在値を
確認してから`changed()`を待つため、terminal通知を取り逃がさない。
sleep/check loopは禁止する。wait timeoutはRunをcancelしない。

## Phase 1のprocess ownership

現行の公式ACP Rust SDKはUnixでproviderを専用process groupに起動し、接続
終了時にgroupをkillしてchildをwaitする。Phase 1はこのguardを利用し、
cleanup完了後にのみterminal receiptを公開する。

親runtimeがSIGKILLされた場合のwatchdog supervisor、TERM/KILL escalation、
Windows Job ObjectはPhase 3で追加する。それまでは「親死亡時のdescendant
回収」やWindowsでの完全なprocess-tree containmentを保証しない。

## Strict continuity（Phase 2）

将来のpersistent Agentは次のstampをRunへ記録する。

```text
provider_session_id
transport_generation
adapter_instance_id
protocol_version
```

`followup`ではprevious Runのstampと現在のstampを一致確認する。processまたは
sessionを失った場合は`continuity_lost`で失敗させ、`session/new`、transcript
replay、別provider、one-shot modeへのfallbackを禁止する。

## Provider方針

provider moduleの責務はexecutable discovery、command args、version probe、
environment allowlist宣言、capability expectation、known incompatibility、
conformance profileだけとする。provider固有CLI protocolの解析は行わない。

導入順はGrok、Cursor、Codex、Claude、Antigravity。Antigravityはcontinuity、
cancel、streamingを同一conformance suiteで満たした場合だけexperimentalとして
登録する。

## 実装フェーズ

1. Phase 0: ID、handle、5-state machine、receipt、failure taxonomy、manifest、ADR
2. Phase 1: Grok one-shot `spawn -> ACP -> wait_run -> cleanup`
3. Phase 2: persistent retention、mailbox、`send`、`followup`、session stamp
4. Phase 3: ACP cancel、deadline、permission cancellation、process supervisor
5. Phase 4: `wait_any/all`、fan-in、race/parallelism tests
6. Phase 5: Cursor、Codex、Claude
7. Phase 6: Antigravity experimental conformance
8. Phase 7: doctor、metrics、soak、release、compatibility matrix

## v0.1で意図的に除外するもの

- CLI protocol/TUI/PTTY scraping
- 実行中turnへの共通steer API
- provider/session間の移行やfallback
- session消失時の自動再作成
- distributed scheduler、Web UI、cloud control plane、HTTP server
- transcript永続化とreplay
- LLM推論速度がRust化で向上するという主張
