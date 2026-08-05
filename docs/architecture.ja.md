# agentmux v2 アーキテクチャ

## 全体像

```text
CLI client
  │ IPC v2 / Unix Domain Socket (0600)
  ▼
Local broker
  ├─ Agent Registry / Run Registry / Wait Hub
  ├─ Global + provider別FIFO scheduler
  ├─ Agent actor（mailbox、active Run、session stamp）
  └─ SQLite v3 metadata/receipt store
       │
       ▼
ProviderDriver → local artifact observation → ACP v1 client
       │
       ▼
internal supervisor → owned provider process group → descendants
```

公開identityはUUIDv7の`AgentId`/`RunId`/`MessageId`だけである。1 Agentは1 logical
worker、1 live process generation、1 ACP transport、1 session、最大1 active Runに対応する。
異なるAgentは並列に動作する。

## ProviderDriverと実行時互換性

4つのbuilt-in `ProviderDriver`がcommand、fixed args/env、environment allowlist、auth、
timeout、ACP version、required capability、identity/artifact probeを所有する。provider version
自体はdriverの実行許可表に含めない。

startupは次の順序で行う。

```text
canonical executable
 → regular/owner/mode/executable safety
 → SHA-256 + dev/inode/size/mtime observation
 → bounded best-effort version/component probes
 → optional exact assertions
 → file identity recheck
 → owned process spawn
 → ACP initialize v1 + capability check
 → authenticate → session/new → prompt
```

probeの`observed`、`unavailable`、`malformed`は監査値であり、対応assertionがない限り
failureではない。assertionはversion、component、launch SHA-256のexact matchのみ。
permission policyはversionから独立する。

`ProviderExecutionIdentity`はprovider、driver ID/revision、target、canonical path、launch
digest、observed values、ACP protocol/agentInfo/capability digest、assertion resultを保持する。
これはtrustやsupportを表さない。continuity fingerprintにはlaunch identity、driver args/env、
permission設定を決定的serializeして使う。

## Run、mailbox、continuity

Run transitionは`queued -> running -> succeeded|failed|interrupted`のみ。stage、failure、
stop reason、cleanupが補助情報を持つ。`send`はAgent actor内のsequence付きmailboxへ追加するだけ。
`followup`受理時のcutoff以前のentryとtaskをbundleし、最新terminal parentと同一sessionへ送る。

process、transport、session、identityが一致しなければcontinuityはlostとなる。provider再起動、
session/new、transcript replay、別provider fallbackによる疑似継続はしない。disk上のbinary更新は
live Agentを変更せず、新規Agentだけが新artifactを観測する。

## Observationとwait

Run event busはbounded sequence付きstreamで、slow consumerにはlag diagnosticを返す。
terminal watchはsubscribe後にcurrent stateを再確認し、通知raceを防ぐ。`wait_any`の同時完了は
global completion sequence、`wait_all`は入力順を使う。wait timeoutはRunをcancelしない。

## Processとhost services

macOSではagentmux自身の`__supervise` modeとwatchdog pipe、専用process groupを用いる。
ACP cancel、grace、stdin close、TERM、KILL、wait/reapの順に終了する。PID名・command名・cwdで
processを探索しない。filesystem/terminal host serviceはcanonical Agent rootからのsymlink escapeを
拒否し、permission policyを適用する。

## 永続化と再起動

SQLite v3はAgent/Run metadata、terminal receipt、completion sequence、provider identityのみを
保存する。prompt、assistant output、reasoning、session secret、credential、environment valueは標準で
保存しない。再起動時のqueued/running Runは`host_restarted`、旧Agentはcontinuity lostとなり、
providerを自動起動しない。

## 境界

通信はlocal UDSだけでHTTP/TCPはない。ACP v2、plugin provider、remote broker、auto update、
session resumeはv2 scope外である。詳細契約は`PRODUCT_CONTRACT.md`、判断はADR-0006を参照する。
