# Codex・Claude・Grok・Cursor統一ランタイム計画

最終更新: 2026-08-05

この文書を`agentmux` v1の実装順序の正本とする。製品契約は
[`PRODUCT_CONTRACT.md`](../PRODUCT_CONTRACT.md)、非目標は
[`NON_GOALS.md`](../NON_GOALS.md)、release gateは
[`V1_DEFINITION_OF_DONE.md`](../V1_DEFINITION_OF_DONE.md)を参照する。

## 1. 完成形

```text
利用者 / 自動化ツール
        │ Control / Observation API
        ▼
┌─────────────────────────────────────┐
│ agentmux: 単一Rust binary           │
│ CLI client / local broker daemon    │
│ Agent・Run runtime / ACP client     │
│ process supervisor / SQLite store   │
└─────────────────────────────────────┘
        │ stable ACP v1 over stdio
        ├── Grok native ACP
        ├── Cursor native ACP
        ├── Codex ACP adapter
        └── Claude ACP adapter
```

`agentmux`自身の配布物は一つだが、provider本体と既存ACP adapterは外部実行
ファイルである。初期完成platformはmacOS arm64、clientとbrokerの通信はUnix
Domain Socketとする。TCP/HTTPは使用しない。

公開provider集合はCodex、Claude、Grok、Cursorの4つに閉じる。内部fake agentは
test fixtureであり、provider enum、manifest、互換性表には現れない。

公式ACP Rust SDKのstable protocol v1を固定versionで使用し、draft protocol用
featureは有効化しない。provider統合は候補経路であり、共通conformance suiteに
合格するまでstableとは呼ばない。

## 2. 固定契約

### Control API

```rust
async fn spawn(request: SpawnRequest) -> Result<SpawnReceipt>;
async fn send(agent: AgentHandle, message: AgentMessage) -> Result<MessageReceipt>;
async fn followup(
    agent: AgentHandle,
    after: RunId,
    task: FollowupTask,
) -> Result<RunHandle>;
async fn interrupt(run: RunHandle) -> Result<InterruptReceipt>;
async fn list(query: ListQuery) -> Result<ListSnapshot>;
```

### Observation API

```rust
async fn wait_run(run: RunHandle, options: WaitOptions) -> Result<RunReceipt>;
async fn wait_any(
    runs: NonEmpty<RunHandle>,
    options: WaitOptions,
) -> Result<RunReceipt>;
async fn wait_all(
    runs: NonEmpty<RunHandle>,
    options: WaitOptions,
) -> Result<Vec<RunReceipt>>;
fn events(run: RunHandle) -> impl Stream<Item = RunEvent>;
```

`events`は読み取り専用であり、control operationには数えない。

### Mailboxとcontinuity

`send`はAgent actor内で順序を確定してmailboxへ格納するだけで、Run作成、ACP
prompt、Agent再開、active Runのsteerを行わない。`followup`受理時にmailboxの
cutoffを固定し、それ以前のentryとtaskを同じACP sessionへ送る。

`followup(agent, after, task)`は、AgentとRunが存在し、`after`がそのAgentの最新
terminal Runで、active/queued Runがなく、processとsession stampが生存している
場合だけ受理する。同時要求はactorへ先に到達した一つだけが受理される。

continuityを失った場合、process再起動、`session/new`、provider切替、transcript
replayを行わない。受理前ならadmission error、受理後なら
`queued -> running -> failed(continuity_lost)`とする。stable ACPにresume機能が
存在しても、v1の保証範囲は同じlive process内だけである。

### 状態と同時実行

```text
queued -> running -> succeeded
                  -> failed
                  -> interrupted
```

上記以外はstateにせず、`RunStage`、`stop_reason`、`failure`、flagsで表す。
Agent用の別status enumも作らない。1 Agentは1 logical worker、1 process
generation、1 ACP session、最大1 active Runであり、異なるAgentは並列実行できる。

## 3. Phase一覧

| Phase | 内容 | 到達点 |
| ---: | --- | --- |
| 0 | Scope・完成条件固定 | 作らないものまで確定 |
| 1 | Domain contract | API、ID、状態機械を固定 |
| 2 | Fake ACP runtime | provider非依存の垂直slice |
| 3 | ACP client・host services | 公式Rust SDKで通信成立 |
| 4 | Owned process lifecycle | 子・孫processを確実に回収 |
| 5 | Grok最小E2E | 最初の実providerが動作 |
| 6 | Streaming・receipt・`wait_run` | Run観測が完成 |
| 7 | `send`・`followup`・continuity | 継続Agentが完成 |
| 8 | `interrupt`・timeout | 中断経路が完成 |
| 9 | Aggregate wait・並列制御 | fan-inと複数Agent完成 |
| 10 | Local broker・CLI・IPC | 別CLI呼び出し間で継続 |
| 11 | Provider conformance suite | provider追加条件を固定 |
| 12 | Cursor統合 | Cursor stable |
| 13 | Codex統合 | Codex stable |
| 14 | Claude統合 | Claude stable |
| 15 | SQLite・再起動処理 | 履歴と異常終了を耐久化 |
| 16 | Security・config・doctor | 運用可能な状態 |
| 17 | 性能・race・soak・chaos | 品質基準を満たす |
| 18 | Packaging・docs・v1 release | 完成 |

## Phase 0 — Scope Freeze

作成物はproduct contract、non-goals、ADR、4 providerだけのcompatibility matrix、
v1 Definition of Doneである。macOS arm64を必須release targetに固定する。

Exit Gate:

- provider集合が4つだけで、catch-allやmaturity用variantがない。
- 将来機能用の空module、互換性row、予約APIがない。
- v1完成条件と非目標が文書化されている。

## Phase 1 — Domain Contract

`AgentId`、`RunId`、`MessageId`、handle、5-state Run、stage、receipt、Agent/Run
snapshot、公開API型、failure taxonomy、transition validatorを実装する。UUIDv7は
path、name、cwdから完全に独立させる。

Run作成前の拒否と、受理後のterminal failureを別の型にする。property testで
許可transitionが4種類だけであること、terminalから遷移しないこと、`send`がRunを
作らず`followup`だけが継続Runを作ることを検証する。

## Phase 2 — Fake ACP Runtime

script可能な内部fake ACP agentで、正常streaming、遅延、permission、prompt失敗、
malformed JSON、transport close、crash、cancel成功/無視、子孫process、session ID
変更、大量出力を再現する。

`AgentRegistry`、`RunRegistry`、`AgentActor`、`RunEventBus`、`WaitHub`、
`GlobalSequence`、内部launcherを実装し、次のsliceを成立させる。

```text
spawn -> queued -> running -> stream -> succeeded -> wait_run
```

このPhaseでは実provider、daemon、SQLiteを導入しない。

## Phase 3 — ACP Client・Host Services

公式ACP Rust SDK stable v1でinitialize、capability negotiation、authenticate、
`session/new`、prompt、update、cancel、permission response、transport/protocol errorを
扱う。JSON-RPC framingは自作しない。

filesystem read/writeとterminal create/output/wait/killをhost serviceとして提供する。
filesystemはcanonicalize・symlink解決後のAgent許可root内だけ、terminal processは
ProcessTreeOwner管理下だけとする。permission待機でACP dispatcherをblockしない。

## Phase 4 — Owned Process Lifecycle

macOSでは同じbinaryを`__supervise` modeで起動し、watchdog pipeと専用process group
を所有する。

```text
broker --watchdog--> agentmux __supervise -> provider process group -> descendants
```

終了はACP cancel、grace、stdin close、TERM、kill grace、KILL、wait/reapの順とする。
process名、cwd、後から推測したPIDでは停止しない。正常終了、crash、broker SIGTERM、
broker強制終了、cancel無視、grandchildをtestし、管理範囲を意図的に離れるadapterは
非対応と判定する。

## Phase 5 — Grok Minimal E2E

Grokだけで次を通す。

```text
spawn -> grok agent stdio -> initialize -> authenticate -> session/new
      -> prompt -> streaming -> wait_run -> receipt -> cleanup
```

one-shot限定で、`send`、`followup`、`interrupt`、aggregate wait、他provider、daemonは
含めない。正常prompt、認証不足、起動失敗、crash、process cleanupをreceiptへ投影し、
provider時間とbroker時間を分離して測定する。

## Phase 6 — Streaming・Receipt・`wait_run`

Run eventはglobal sequence、Run/Agent ID、timestamp、共通kind、provider metadataを
持つ。output、reasoning、tool lifecycle、permission、diagnosticを順序付きで配信する。

terminal receiptはstate、stop reason、failure、timestamps、bounded output、metrics、
cleanupを持つ。`wait_run`はsubscriber登録後にcurrent snapshotを再確認し、pollingせず
terminalを待つ。wait timeoutはRunを中断しない。slow clientがstreamを読まなくても
provider処理がdeadlockせず、bufferが無制限に増えないことをtestする。

## Phase 7 — Persistent Agent・`send`・`followup`

Agentをpersistent化し、sequence付きmailboxと次のsession stampを各Runへ記録する。

```text
provider_session_id
transport_generation
adapter_instance_id
provider_profile_fingerprint
```

continuity loss reasonはprovider/transport exit、session change、adapter restart、idle
expiry、forced kill、host restart、protocol corruptionである。明示`close`は追加せず、
configurable idle TTL、broker shutdown、crash、force kill、回復不能protocol errorだけが
Agent processを終了する。3 turn以上、mailbox cutoff race、stale parent、no fallback、
graceful interrupt後のfollowupをtestする。

## Phase 8 — `interrupt`・Timeout

interrupt requestはRun actorで直列化し、ACP cancelを試み、grace超過時だけowned
process escalationを行う。terminal確定後はidempotent no-op、正常完了が先なら
`succeeded`、cancelledなら`interrupted`、crashなら`failed`とする。killが必要なら
continuityを失う。

execution deadlineは同じinterrupt pathを使い、`state = interrupted`、
`stop_reason = deadline_exceeded`とする。stream、tool、permission、queued Run、連打、
completion race、cleanupをtestする。

## Phase 9 — `wait_any`・`wait_all`・Concurrency

`wait_any`は空集合を拒否し、同時terminal時はglobal completion sequenceで最初を
決定する。他Runはcancelしない。`wait_all`は一部failedでも全terminalを待ち、入力順で
返す。いずれもwait timeoutでRunをcancelしない。

schedulerはglobal limit、provider別limit、1 Agent 1 active Run、FIFO admissionを
持つ。100以上のfake Agent、registration race、capacity release、cross-Agent並列性を
busy pollingなしで検証する。

## Phase 10 — Local Broker・IPC・CLI

同一binaryに次のmodeを持たせる。

```text
agentmux serve | spawn | send | followup | interrupt | list
agentmux wait | watch | doctor | __supervise
```

macOSのUnix Domain Socketはcurrent user所有、mode 0600とし、single-broker lock、
protocol version handshake、request ID、bounded frameを実装する。CLI終了後もRunが
継続し、別CLIから同じAgentを操作できること、client切断がRunを中断しないこと、
二重起動とversion mismatchを明示拒否すること、JSON出力をtestする。

## Phase 11 — Provider Conformance Suite

全providerにinitialize、auth path、session creation、first prompt、stream order、
3 turn、cancel/escalation、crash、malformed protocol、no fallback、child/grandchild
cleanup、output boundary、exact version testを適用する。permission、filesystem、
terminal、images、MCP、resumeはadvertiseされた場合だけ必須とする。

provider追加で変更できるのはmanifest、version probe、auth configuration、capability
expectation、metadata projection、known incompatibility、fixtureに限定する。core state、
identity、mailbox、followup、wait、continuity契約は変更しない。Grok合格を最初のstable
認定とする。

## Phase 12 — Cursor Integration

native `agent acp`を使い、executable discovery、version probe、既存account auth、
stderr、capability snapshot、profile、fixtureだけを追加する。内部protocolを解析せず、
model listを共通必須APIにせず、advertiseされない機能を利用しない。mandatory suite
合格後にstableとする。

## Phase 13 — Codex Integration

exact versionの`@agentclientprotocol/codex-acp`を使う。adapter discovery、bundled互換
Codex、ChatGPT/API-key auth、approval/sandbox/subagent metadata、crash監視、fixtureを
追加する。別Codex binaryは明示設定時だけ許可し、自動fallbackしない。

Codex App Server protocolやCLI JSONを再実装せず、Codex専用Run stateを作らず、
subagentを独立Agentへ昇格せず、固有steerを共通`send`へ混ぜない。初回Run、same-session
followup、approval allow/deny、interrupt、crash、forced cleanup、no fallbackを含む
mandatory suite合格後にstableとする。

## Phase 14 — Claude Integration

exact versionの`claude-agent-acp`を使い、discovery、Claude auth、permission、terminal、
nested subagent metadata、crash監視、fixtureを追加する。Claude固有protocolやtranscript
を直接読まず、nested subagentを独立Agent化せず、background taskを共通Runへ再定義せず、
permissionを自動許可しない。mandatory suite合格後にstableとする。

## Phase 15 — SQLite・Broker再起動処理

Agent/Run ID、provider、display metadata、state、timestamps、failure、stop reason、
terminal receipt、completion sequence、tested versionsだけを保存する。prompt、output、
reasoning、full tool args、secret、session secret、environment valueは標準保存しない。

broker起動時にpersisted `queued`/`running` Runを`failed(host_restarted)`へ確定し、
以前のAgentを`continuity = lost(host_restarted)`にする。provider起動やsession recoveryは
行わない。receipt再読込、migration、DB破損時の安全停止、transcript非保存をtestする。

## Phase 16 — Security・Configuration・Doctor

provider固有設定はprofileへ隔離する。environment allowlist、secret redaction、root
canonicalization、symlink escape防止、IPC 0600、frame/output/stderr bound、permission
deadline、unattended default deny、checksum/exact version、opt-in content loggingを実装する。
provider processのnetwork挙動を共通保証しない。

`doctor`はbroker/ACP/provider/adapter version、path、auth availability、capability、socket
permission、SQLite schema、lifecycle fixture、既知非互換を人間向けとJSONで報告する。
自動修正と自動updateは行わない。

## Phase 17 — Performance・Race・Soak・Chaos

性能はagentmux overhead、provider startup、ACP init、auth、model/tool execution、cleanupを
分離する。reference hostのrelease buildで次を初期budgetとする。

| 指標 | 目標 |
| --- | ---: |
| cold startup p95 | 50 ms以下 |
| broker idle RSS | 30 MiB以下 |
| in-process admission p99 | 2 ms以下 |
| IPC admission p99 | 5 ms以下 |
| terminal eventからwaiter wake p99 | 5 ms以下 |
| synthetic event fan-in | 10,000 events/secで欠落なし |
| fake lifecycle | 10,000 Runで不整合なし |
| real lifecycle | 1,000回でzombie/FD leakなし |

budget変更には測定結果とADRを必要とする。interrupt/completion、mailbox cutoff、二重
followup、exit/terminal、wait registration、shutdown/startup、TTL/followup、SQLite
commit/crashのraceを検証する。protocol破損、stderr flood、hang、cancel無視、auth timeout、
permission切断、disk full、socket切断、broker terminationのchaosを行う。

Exit Gateはstate invariant違反、orphan、通知欠落、unbounded queue、broker全体panicが
ゼロで、budget達成と4 provider smoke test合格である。

## Phase 18 — Packaging・Documentation・v1 Release

macOS arm64 binary、checksum、SBOM、third-party license、versioned compatibility
manifest、Homebrew等の導入手段、reproducible手順、signed artifactを用意する。

README、installation、provider/auth setup、CLI/Rust API、state machine、continuity、
mailbox、interrupt、permission、process ownership、compatibility、troubleshooting、security、
performance、upgrade policyを完成させる。最終判定は
[`V1_DEFINITION_OF_DONE.md`](../V1_DEFINITION_OF_DONE.md)だけを基準にする。

## 4. 依存関係と開発規則

```text
Phase 0 -> 1 -> 2 -> 3 -> 4 -> 5 -> 6 -> 7 -> 8 -> 9 -> 10 -> 11
                                                               ├-> 12
                                                               ├-> 13
                                                               └-> 14
                                                                    ↓
                                                              15 -> 16 -> 17 -> 18
```

Phase 12〜14はPhase 11後に並列開発できるが、一つずつmergeして毎回core regressionを
通す。各Phaseは独立Epic/PRとし、次Phaseのコードを先回りしない。Grok slice前に
provider abstractionを一般化し過ぎず、conformance suite前に他providerを実装しない。
provider追加でcore stateを変えず、存在しない将来provider向けextension pointを作らず、
性能最適化は測定後だけ行い、Exit Gate到達後はそのPhaseの改善を止める。

## 5. Primary references

- [ACP Rust SDK](https://github.com/agentclientprotocol/rust-sdk)
- [ACP session resume announcement](https://agentclientprotocol.com/announcements/session-resume-stabilized)
- [Grok headless/ACP documentation](https://docs.x.ai/build/cli/headless-scripting)
- [Cursor ACP documentation](https://cursor.com/docs/cli/acp)
- [Codex ACP adapter](https://github.com/agentclientprotocol/codex-acp)
- [Codex App Server](https://learn.chatgpt.com/docs/app-server)
- [Claude ACP adapter](https://github.com/agentclientprotocol/claude-agent-acp)
