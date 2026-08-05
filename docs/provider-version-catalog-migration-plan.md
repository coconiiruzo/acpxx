# agentmux Provider Version Catalog 移行完了計画

> Status: Proposed
> Target release: `agentmux 2.0.0`
> Baseline: `main`（2026-08-05 確認）
> Intended repository path: `docs/provider-version-catalog-migration-plan.md`

## 1. 目的

現在の agentmux は、Grok、Cursor、Codex ACP、Claude ACP の検証済みバージョンを agentmux バイナリと設定スキーマに直接固定している。この方式は再現性と安全性には優れるが、provider の更新だけでも agentmux 本体の変更・再テスト・再リリースが必要になり、provider の更新頻度が agentmux のリリース頻度を支配する。

本計画では、次の状態まで移行を完了する。

1. provider の起動・認証・権限・process supervision 契約は、引き続き agentmux 内蔵の `ProviderDriver` が所有する。
2. 検証済み provider version と artifact identity は、agentmux バイナリから独立して更新可能な署名済み Compatibility Catalog が所有する。
3. Agent 初期化時に、実際の provider identity を exact な `ResolvedProviderLock` に解決する。
4. 解決後の Agent は、Catalog 更新や provider executable の更新から影響を受けない。
5. ユーザーは agentmux 本体を更新せず、Catalog に追加された新しい検証済み provider version を利用できる。
6. 未検証最新版を必要とするユーザーには、明示的かつ制限された `experimental` policy を提供する。
7. provider binary や adapter の自動インストール・自動更新は行わない。

この移行は、単に pin を撤廃するものではない。**複数の検証済み exact pin を署名済み Catalog で配布し、pin の確定地点を agentmux release から個々の Agent へ移す**。

---

## 2. バージョニング判断

### 2.1 推奨ターゲット

`agentmux 2.0.0` としてリリースする。

理由は以下のとおり。

- provider profile の設定スキーマを変更する。
- `ProviderSnapshot`、`AgentSnapshot`、`RunReceipt` に provider lock と Catalog 状態を導入する。
- Rust 公開型と IPC wire contract を変更する可能性が高い。
- 現在の upgrade policy は、公開契約の非互換変更を major release として扱う。

### 2.2 リリース列

実装中の公開確認には以下を使用する。

```text
2.0.0-alpha.1  Catalog model / bootstrap / shadow resolution
2.0.0-beta.1   signed update / config v2 / runtime cutover
2.0.0-rc.1     all-provider qualification / docs / packaging
2.0.0          complete migration
```

バージョン番号は工程の進捗を表すだけで、Catalog sequence と provider version の更新履歴には使用しない。

---

## 3. 現状と移行対象

### 3.1 現在の固定点

現状では、主に以下が provider version を agentmux release に結合している。

| 固定点 | 現在の責務 | 移行後 |
|---|---|---|
| `src/providers/*.rs` の `*_TESTED_VERSION` | 実行可能 version の唯一の許可値 | bootstrap fixture または削除 |
| `VersionProbe::{Semver, ExactOutput}` の requirement/expected | version 取得と許可判定を同時に実施 | identity の観測だけを実施 |
| `src/config.rs` の exact version 比較 | profile 値を compile-time constant と比較 | policy と Catalog resolver へ委譲 |
| profile 内の `version` / `sha256` / component version | ユーザーが互換性 evidence を自己申告 | Catalog entry または resolver が導出 |
| `initialize_verified` / `session_new_verified` | ユーザーが conformance を boolean で申告 | Catalog qualification evidence が所有 |
| `compatibility/agentmux-1.0.0.json` | agentmux 1版に provider 1組 | 複数 version を収録する独立 Catalog |
| `tests/release_manifest.rs` | Cargo version と manifest filename を結合 | binary release と Catalog release を別々に検証 |
| `docs/release.md` | provider 更新を binary release gate に含める | binary gate と Catalog gate を分離 |

### 3.2 既存の安全性契約で維持するもの

以下は移行後も変更しない。

- 対象 provider は Codex、Claude、Grok、Cursor の4種に限定する。
- ACP stable v1 を使用する。
- provider fallback を行わない。
- provider stdout は ACP 専用とする。
- environment は Driver の allowlist のみ渡す。
- mutation permission は deny が既定値である。
- process tree は agentmux が所有し、interrupt/shutdown 後に回収する。
- startup failure は通常の terminal `RunReceipt` 経路で返す。
- provider executable / adapter を spawn 中に自動取得・自動更新しない。
- 実行中 Agent の process/session continuity を維持する。

---

## 4. スコープ

### 4.1 対象

- 内蔵 `ProviderDriver` と provider identity probe の分離
- Compatibility Catalog schema
- Catalog の署名、検証、cache、rollback protection、expiry
- `verified` / `exact` / `experimental` version policy
- Agent 初期化時の provider resolution
- immutable `ResolvedProviderLock`
- config v2 と config migration command
- Agent/Run persistence、Rust API、IPC、CLI 出力の更新
- `doctor`、Catalog status/update、provider verify command
- provider qualification evidence と Catalog publish workflow
- all-provider conformance、security、race、soak、release gate
- README、provider setup、troubleshooting、upgrade/release policy の全面更新

### 4.2 非対象

- provider plugin marketplace
- arbitrary command provider
- Catalog が command、args、environment、permission policy を配布する仕組み
- provider binary / adapter の package manager
- background updater、filesystem watcher、spawn 時の network access
- provider fallback、session replay、別 provider への自動切替
- ACP v2 対応
- 実行中 Agent の自動再起動・強制停止
- semver range を upstream version 全体へ直接適用すること

将来 side-by-side provider installer を追加する場合も、本計画で定義する Catalog と lock を再利用し、別プロジェクトとして扱う。

---

## 5. 完成後のアーキテクチャ

```text
Provider upstream release
        │
        ▼
Candidate discovery
        │
        ▼
Authenticated qualification
  - identity / artifact capture
  - ACP initialize
  - session/new
  - streaming order
  - multi-turn continuity
  - cancel recovery
  - permission audit
  - process-tree audit
  - capability inventory
        │
        ▼
Maintainer approval
        │
        ▼
Signed Compatibility Catalog
        │ explicit update only
        ▼
Local verified Catalog cache
        │
        ▼
Profile version policy
        │
        ▼
Agent startup resolver
  Driver + observed identity + artifact digest + Catalog
        │
        ▼
ResolvedProviderLock
        │
        ├── AgentSnapshot / persistence
        ├── RunReceipt / IPC
        └── process/session fingerprint
```

### 5.1 責務境界

#### ProviderDriver

agentmux バイナリにコンパイルされる。以下を所有する。

- provider ID
- Driver ID と revision
- launch command contract
- fixed args
- environment allowlist / fixed environment
- authentication discovery contract
- permission translation
- ACP transport
- identity probe の実装
- artifact identity の算出方法
- process supervision と provider-specific workaround

Catalog は `driver_id` と `driver_revision` を参照できるが、Driver の command、args、environment を上書きできない。

#### Compatibility Catalog

データとして以下を所有する。

- 検証済み exact provider identity
- 対象 platform
- 対応する Driver ID/revision
- artifact digest
- agentmux compatibility range
- ACP protocol
- qualification suite version と evidence digest
- lifecycle state（`verified` / `deprecated` / `blocked`）
- provider/target ごとの recommended entry

#### ResolvedProviderLock

Agent 初期化時に一度だけ生成し、その Agent の生存期間中は変更しない。

---

## 6. データ契約

### 6.1 ProviderDriver

概念上の最終形を以下とする。

```rust
pub struct ProviderDriver {
    pub provider: ProviderId,
    pub driver_id: DriverId,
    pub driver_revision: u32,
    pub launch: LaunchContract,
    pub identity_probe: IdentityProbe,
    pub artifact_probe: ArtifactProbe,
    pub protocol: ProtocolLock,
    pub capabilities: BrokerCapabilityContract,
    pub environment: EnvironmentContract,
    pub authentication: AuthenticationContract,
    pub startup_timeout: Duration,
}
```

`ProviderSpec::manifest()` が compile-time exact version を返す構造は廃止し、Driver contract を返す構造へ変更する。

### 6.2 ProviderIdentity

version 表記を semver に限定しない。

```rust
pub struct ProviderIdentity {
    pub display_version: String,
    pub normalized_version: String,
    pub components: BTreeMap<String, String>,
}
```

- Grok: CLI version
- Cursor: opaque version string
- Codex: adapter versionと、観測可能な Codex component identity
- Claude: adapter versionと、観測可能な Agent SDK component identity

component version が直接 probe できない場合、設定ファイルの自己申告を信用しない。署名対象 package metadata または artifact digest に覆われた metadata から取得する。

### 6.3 ArtifactIdentity

単一 executable の SHA-256 に限定せず、Driver が検証対象を宣言する。

```rust
pub struct ArtifactDigest {
    pub subject: String,
    pub algorithm: DigestAlgorithm,
    pub digest: String,
}
```

例:

- native CLI executable の `sha256`
- adapter entrypoint の `sha256`
- package metadata / bundled component manifest の digest

少なくとも launch artifact を覆う digest を必須とする。Catalog entry と local artifact identity が一致しない場合、version string が一致しても `verified` として実行しない。

> **移行上の注意:** 現行 v1 の profile にある `sha256` は、ユーザーが local executable に対して設定する local pin であり、現在の compatibility manifest には中央で検証済みの artifact digest 一覧が存在しない。したがって bootstrap Catalog は version 文字列だけから機械的には完成しない。runtime cutover 前に、provider/target/install artifact ごとの qualification を実行して accepted digest を収集する。既存 v1 profile の digest が Catalog に存在しない場合、migration command は自動承認せず、qualified artifact の再導入または `experimental` policy を案内する。この major release では、version-only compatibility から qualified artifact compatibility へ意図的に trust を強化する。

### 6.4 Compatibility Catalog

Catalog 本体は exact bytes を detached signature の対象とする。JSON canonicalization に依存しない。

```json
{
  "schema_version": 1,
  "catalog_id": "agentmux-official",
  "sequence": 42,
  "generated_at": "2026-08-05T00:00:00Z",
  "expires_at": "2026-09-05T00:00:00Z",
  "entries": [
    {
      "entry_id": "grok/0.2.118/aarch64-apple-darwin/sha256-...",
      "provider": "grok",
      "target": "aarch64-apple-darwin",
      "driver_id": "grok-native",
      "driver_revision": 1,
      "agentmux_requirement": ">=2.0.0 <3.0.0",
      "protocol": "v1",
      "identity": {
        "display_version": "0.2.118",
        "normalized_version": "0.2.118",
        "components": {}
      },
      "artifacts": [
        {
          "subject": "executable",
          "algorithm": "sha256",
          "digest": "..."
        }
      ],
      "state": "verified",
      "not_after": null,
      "qualification": {
        "suite_version": 1,
        "tested_at": "2026-08-05T00:00:00Z",
        "evidence_digest": "sha256:..."
      }
    }
  ],
  "channels": [
    {
      "provider": "grok",
      "target": "aarch64-apple-darwin",
      "name": "recommended",
      "entry_id": "grok/0.2.118/aarch64-apple-darwin/sha256-..."
    }
  ]
}
```

#### Catalog validation rules

- `schema_version` は既知の値だけを受理する。
- unknown field は拒否する。
- `catalog_id` と署名 keyring の対応を検証する。
- `sequence` は、同一 `catalog_id` で受理済みの最大値より大きい必要がある。
- `expires_at` を過ぎた remote Catalog は、新規 `verified` / `exact` Agent の解決に使わない。
- `entry_id`、identity、channel に曖昧な重複を許さない。
- channel は同一 provider/target の有効 entry のみ参照できる。
- `blocked` が常に優先される。
- `deprecated` は `not_after` まで受理し、warning を出す。
- target、Driver ID/revision、agentmux requirement、protocol が一致しない entry は利用しない。
- Catalog 内に command、args、environment、path、permission policy を持たせない。

### 6.5 Catalog signature envelope

```json
{
  "schema_version": 1,
  "catalog_sha256": "...",
  "signatures": [
    {
      "key_id": "agentmux-catalog-2026-01",
      "algorithm": "ed25519",
      "signature": "base64..."
    }
  ]
}
```

- agentmux binary は信頼済み public keyring を内蔵する。
- rotation に備えて複数署名を受理できる schema にする。
- private key は repository、release artifact、通常 CI runner に置かない。
- protected environment または KMS による署名を使用する。
- Sigstore provenance/SBOM は補助 evidence として維持するが、runtime の offline 検証は detached Ed25519 signature を基準とする。

### 6.6 ResolvedProviderLock

```rust
pub struct ResolvedProviderLock {
    pub provider: ProviderId,
    pub driver_id: DriverId,
    pub driver_revision: u32,
    pub target: String,
    pub compatibility: CompatibilityLevel,
    pub identity: ProviderIdentity,
    pub artifacts: Vec<ArtifactDigest>,
    pub catalog_entry_id: Option<String>,
    pub catalog_sequence: Option<u64>,
    pub catalog_digest: Option<String>,
}

pub enum CompatibilityLevel {
    Verified,
    Deprecated,
    Experimental,
}
```

`experimental` では Catalog fields が `None` になる。既知の `blocked` identity は `experimental` でも拒否する。

lock の canonical serialization digest を、process/session continuity fingerprint に含める。既存の `expected_version` 文字列だけに依存した fingerprint は廃止する。

---

## 7. Version policy

### 7.1 `verified` — 既定値

```toml
[profiles.grok-default]
provider = "grok"
executable = "/opt/homebrew/bin/grok"
version_policy = "verified"
permissions = "deny"
```

動作:

1. executable と identity を probe する。
2. artifact digest を算出する。
3. 現在の有効 Catalog から exact match を探す。
4. `verified` または期限内 `deprecated` entry なら lock を生成する。
5. 未登録、hash mismatch、blocked、expired Catalog なら fail closed とする。

`verified` は upstream の「最新版」を自動選択しない。ユーザーがインストール済み executable を更新し、その exact identity が Catalog に登録済みなら利用可能になる。

### 7.2 `exact` — CI、再現実行、固定環境

```toml
[profiles.grok-ci]
provider = "grok"
executable = "/opt/agents/grok/0.2.118/grok"
version_policy = "exact"
catalog_entry = "grok/0.2.118/aarch64-apple-darwin/sha256-..."
permissions = "deny"
```

動作:

- 指定 `catalog_entry` が有効 Catalog に存在することを要求する。
- local identity と artifact digest が entry に完全一致することを要求する。
- entry が deprecated なら warning、blocked/expired なら拒否する。
- version string だけの pin は許可しない。

### 7.3 `experimental` — 未検証最新版の明示利用

```toml
[profiles.grok-preview]
provider = "grok"
executable = "/opt/homebrew/bin/grok"
version_policy = "experimental"
permissions = "deny"
```

動作:

- identity と artifact digest は必ず観測し、lock に記録する。
- Catalog に exact verified match がある場合は自動的に `Verified` として扱う。
- exact match がない場合のみ `Experimental` とする。
- ACP initialize/session negotiation は通常どおり必要とする。
- known blocked identity は拒否する。
- output、snapshot、receipt に unverified warning を残す。
- mutation は既定で強制 deny とする。
- mutation を許可する場合は、profile の `permissions = "allow-all"` と spawn 時の `--allow-unverified-mutations` の二重 opt-in を要求する。
- 実行結果から Catalog を自動更新しない。

### 7.4 禁止する policy

以下は実装しない。

```text
>=0.2.118
latest
latest-upstream
any-v1
```

semver/opaque version の大小ではなく、Catalog 内の exact entry と明示 channel を source of truth とする。

---

## 8. Resolver の実行順序

Agent actor が初回 Run の capacity を取得した後、provider process を spawn する前に以下を行う。現在の「spawn は handle を返し、startup failure は terminal receipt で返す」契約は維持する。

```text
1. profile を config v2 として読み込む
2. provider に対応する built-in Driver を選択する
3. executable/adapter path を canonicalize する
4. regular file、owner、mode、symlink policy を検証する
5. Driver の IdentityProbe で identity を観測する
6. Driver の ArtifactProbe で digest を算出する
7. CatalogStore から現在の immutable Catalog snapshot を取得する
8. blocked entry を deny-first で評価する
9. version policy を適用する
10. ResolvedProviderLock を生成する
11. launch 直前に file identity を再検証する
12. process を spawn する
13. ACP initialize / session/new を実行する
14. lock と capabilities を Agent snapshot に確定する
```

### 8.1 TOCTOU 境界

- hash 対象 file descriptor から device/inode/size/mtime を取得する。
- spawn 直前に path を再 stat し、不一致なら失敗させる。
- adapter bundle では Driver が覆う metadata/digest の集合を明示する。
- 同一ユーザー権限を持つ敵対 process に対する完全防御は本 threat model に含めないが、通常の path replacement と自動更新 race は検出する。
- provider が自己更新を無効化できる場合、Driver の fixed args で無効化を継続する。

### 8.2 Catalog reload

- `serve` / `spawn` は network access を行わない。
- `agentmux compatibility update` だけが remote/file Catalog を取得する。
- daemon 稼働中は、update 完了後に IPC で `reload_compatibility` を要求する。
- daemon は cache を再読込・再検証し、`Arc<Catalog>` を atomic に交換する。
- 既に resolution を開始した Agent は取得済み Catalog snapshot を使い切る。
- 既存 Agent とその followup は既存 lock/process/session を使い続ける。

---

## 9. Catalog cache と offline 動作

### 9.1 local layout

```text
$XDG_DATA_HOME/agentmux/compatibility/
  generations/<sequence>-<digest>/catalog.json
  generations/<sequence>-<digest>/catalog.sig
  state.json
  state.lkg.json
```

macOS 等で `XDG_DATA_HOME` がない場合は、既存 agentmux directory policy と統一した fallback を使う。

### 9.2 更新アルゴリズム

1. download size 上限を適用して temporary file へ保存する。
2. signature envelope と Catalog exact bytes を検証する。
3. schema、target、time、agentmux requirement、entry/channel integrity を検証する。
4. `sequence > highest_accepted_sequence` を確認する。
5. immutable generation directory へ書き込み、file と directory を fsync する。
6. `state.json` が新 generation と highest sequence/digest を参照するよう atomic replace する。
7. 同一 highest state を `state.lkg.json` に冗長保存する。以前の sequence を automatic LKG として再有効化しない。
8. daemon があれば local cache の reload を通知する。

### 9.3 bootstrap Catalog

- `agentmux 2.0.0` binary に、現在の4 provider exact identity と、release qualification で取得した supported artifact digest を含む bootstrap Catalog と signature を埋め込む。
- 初回起動や cache 未作成時は bootstrap を使用する。
- bootstrap は移行前と同じ version/Driver の behavioral allow set を表す。一方、artifact は中央で qualification された digest に限定するため、任意の user-supplied v1 checksum まで自動承認するものではない。
- 一度 higher sequence の remote Catalog を受理した環境では、bootstrap への暗黙 rollback を行わない。

### 9.4 障害時

| 状態 | 動作 |
|---|---|
| download failure | 現在の highest accepted generation が fresh なら継続し、update command は non-zero |
| signature invalid | 新 Catalog を破棄し、state pointer を変更しない |
| lower/equal sequence | rollback/replay として拒否 |
| active generation corruption | 同一 sequence/digest を指す冗長 state/generation のみ使用。以前の sequence へ自動 rollback しない |
| remote Catalog expired | 既存 Agent は継続、新規 verified/exact Agent は拒否 |
| cache 未作成 | bootstrap を使用 |
| higher sequence 受理後に cache 全損 | 安全側で停止し、signed Catalog の再取得を要求 |

air-gapped 環境では `compatibility update --file ... --signature ...` により署名済み Catalog を搬入する。

---

## 10. Config v2

### 10.1 新スキーマ

```toml
schema_version = 2

[catalog]
source = "official"

[profiles.grok-default]
provider = "grok"
executable = "/opt/homebrew/bin/grok"
version_policy = "verified"
permissions = "deny"

[profiles.cursor-default]
provider = "cursor"
executable = "/absolute/path/to/cursor-agent"
version_policy = "verified"
permissions = "deny"

[profiles.codex-default]
provider = "codex"
adapter_path = "/absolute/path/to/codex-acp"
version_policy = "verified"
permissions = "deny"

[profiles.claude-default]
provider = "claude"
adapter_path = "/absolute/path/to/claude-agent-acp"
version_policy = "verified"
permissions = "deny"
```

file source の例:

```toml
[catalog]
source = "file"
path = "/opt/agentmux/catalog/catalog.json"
signature_path = "/opt/agentmux/catalog/catalog.sig"
```

### 10.2 削除する user-authored fields

- `args`
- `version`
- `sha256`（exact entry 内へ移動）
- `adapter_version`
- `codex_version`
- `claude_agent_sdk_version`
- `authentication`
- `initialize_verified`
- `session_new_verified`

これらは Driver、Catalog、runtime probe、qualification evidence が所有する。

### 10.3 migration command

```bash
agentmux config migrate --check
agentmux config migrate --write
```

要件:

- v1 profile を全 provider variant で parse する。
- version/component/hash を bootstrap/active Catalog entry と照合する。
- exact match があれば `version_policy = "exact"` と `catalog_entry` へ変換する。
- path と permission policy を保持する。
- Driver fixed args と一致しない旧 `args` は自動変換せず、profile 単位の actionable error とする。
- self-attested evidence fields は警告を出して削除する。
- `--check` は変更を行わず、diff と blocking issue を表示する。
- `--write` は timestamp 付き backup を作成し、mode `0600` を維持して atomic replace する。
- normal `serve` / `spawn` は v1 config を黙って解釈しない。migration command を示す明確な error を返す。
- docs のすべての TOML example を parser test で検証する。

---

## 11. CLI と診断契約

### 11.1 追加 command

```bash
agentmux compatibility status [--json]
agentmux compatibility update [--json]
agentmux compatibility update --file PATH --signature PATH [--json]
agentmux provider status [--profile NAME] [--json]
agentmux provider verify PROFILE [--json]
agentmux config migrate --check
agentmux config migrate --write
```

### 11.2 `compatibility status`

最低限、以下を返す。

- source（bootstrap / cache / file）
- catalog ID、sequence、digest
- generated/expiry
- signature key ID
- highest accepted sequence
- LKG の状態
- provider/target ごとの recommended entry
- binary update が必要な unsupported Driver revision entry

### 11.3 `provider verify`

process を起動せず、以下を実施する。

- path security validation
- identity probe
- artifact digest
- Catalog match
- blocked/deprecated 判定
- exact entry ID
- experimental 判定時の warning

### 11.4 `doctor`

既存 doctor に以下を追加する。

- config schema version
- Catalog signature/sequence/expiry/cache permission
- profile ごとの installed identity
- selected compatibility state
- recommended identity との差
- Driver revision compatibility
- artifact mismatch の対象 subject
- migration 未実施 config の具体的修正 command

秘密情報、prompt、output、auth token は出力しない。

---

## 12. Persistence、API、IPC

### 12.1 AgentSnapshot

- startup resolution 前は `provider_lock = None` を許容する。
- process spawn 前に lock を確定する。
- successful startup 後の Agent では lock を必須とする。
- followup では同一 lock digest を要求する。

### 12.2 RunReceipt

standalone で監査可能にするため、以下の summary を含める。

```rust
pub struct ProviderLockSummary {
    pub provider: ProviderId,
    pub compatibility: CompatibilityLevel,
    pub display_version: String,
    pub components: BTreeMap<String, String>,
    pub artifact_digests: Vec<ArtifactDigest>,
    pub driver_id: DriverId,
    pub driver_revision: u32,
    pub catalog_entry_id: Option<String>,
    pub catalog_sequence: Option<u64>,
    pub catalog_digest: Option<String>,
}
```

startup resolution failure では、lock を偽造せず、failure detail に observed identity と rejection reason を記録する。

### 12.3 SQLite migration

- storage schema を v2 へ上げる。
- JSON snapshot に新 field を追加する場合も、migration version を明示する。
- v1 record は `provider_lock = None` / `legacy_unknown` として読み込む。
- 現在の Catalog から過去の lock を推測・補完しない。
- v2 record の lock round-trip、redaction、restart をテストする。
- prompt/output persistence の既存 redaction contract を変更しない。

### 12.4 Rust API

- provider compatibility 用の公開型を `lib.rs` から export する。
- 将来 field 追加が想定される公開 struct/enum には、利用形態を確認したうえで `#[non_exhaustive]` を適用する。
- 旧 `expected_version` の意味を残さず、installed/selected/recommended/compatibility を明示する。

### 12.5 IPC

- wire protocol を v2 へ上げる。
- v1 client と v2 daemon、v2 client と v1 daemon は明示的 version mismatch を返す。
- Catalog reload、status、provider verify を IPC operation に追加する。
- unknown field の黙殺に依存せず、contract test を更新する。

---

## 13. 実装フェーズと PR 分割

すべての PR は単独で format、clippy、全自動テストを通し、未完成の runtime fallback を導入しない。

| PR | 内容 | 依存 | runtime authority |
|---|---|---|---|
| PR-00 | ADR、契約、schema freeze | なし | legacy pin |
| PR-01 | Catalog domain、bootstrap、parser | PR-00 | legacy pin |
| PR-02 | signature、cache、update store | PR-01 | legacy pin |
| PR-03 | Driver / identity / artifact probe 分離 | PR-01 | legacy pin |
| PR-04 | resolver と version policy、shadow comparison | PR-02, PR-03 | legacy pin |
| PR-05 | config v2 と migration command | PR-04 | legacy pin |
| PR-06 | Agent startup cutover と immutable lock | PR-05 | Catalog resolver |
| PR-07 | persistence、Rust API、IPC v2 | PR-06 | Catalog resolver |
| PR-08 | CLI status/update/verify、doctor | PR-07 | Catalog resolver |
| PR-09 | qualification evidence と publish workflow | PR-08 | Catalog resolver |
| PR-10 | all-provider canary、docs、packaging | PR-09 | Catalog resolver |
| PR-11 | legacy pin cleanup、release gate、2.0.0 | PR-10 | Catalog resolver only |

---

## 14. PR 詳細

### PR-00: ADR と契約固定

#### 変更

- `docs/adr/0002-provider-compatibility-catalog.md` を追加する。
- 本計画を `docs/provider-version-catalog-migration-plan.md` に配置する。
- threat model、trust root、expiry、rollback、experimental permission を ADR で決定する。
- Catalog schema v1 と config schema v2 の変更ルールを決める。
- `PRODUCT_CONTRACT.md` と `NON_GOALS.md` に、Catalog metadata update は provider auto-update ではないことを明記する。

#### Gate

- Driver と Catalog の責務が重複していない。
- Catalog が arbitrary command を配布できないことが契約化されている。
- binary release が必要な変更と Catalog-only update の境界が明文化されている。

#### Rollback

文書のみ。コードへの影響なし。

---

### PR-01: Catalog domain、schema、bootstrap

#### 新規候補

```text
src/compatibility/mod.rs
src/compatibility/catalog.rs
src/compatibility/model.rs
compatibility/schema/catalog-v1.schema.json
compatibility/bootstrap/catalog-v1.json
compatibility/bootstrap/catalog-v1.sig
tests/compatibility_catalog.rs
```

#### 変更

- strict serde model を実装する。
- duplicate、invalid channel、unsupported target/driver、invalid timestamp を拒否する validator を実装する。
- 現在の4 provider exact identity を bootstrap Catalog へ移す。
- bootstrap bytes を `include_bytes!` で埋め込む。
- 旧 `compatibility/agentmux-1.0.0.json` は historical data として保持し、runtime source ではないことを明記する。

#### Tests

- valid bootstrap parse
- unknown field rejection
- unsupported schema
- duplicate entry/channel
- malformed identity/artifact
- agentmux requirement mismatch
- blocked/deprecated state validation

#### Gate

bootstrap Catalog の version/Driver behavioral allow set が旧 compile-time pin と一致することを fixture test で証明する。加えて、Catalog に収録されていない v1 local checksum は自動承認されず、明示的な migration error になることを証明する。

#### Rollback

runtime 未接続のため、ファイル追加を revert できる。

---

### PR-02: Signature、cache、rollback protection

#### 新規候補

```text
src/compatibility/signature.rs
src/compatibility/cache.rs
src/compatibility/store.rs
tests/catalog_security.rs
```

#### 変更

- embedded public keyring と detached signature 検証を実装する。
- exact-byte SHA-256 と Ed25519 verification を実装する。
- Catalog size 上限、strict parse、expiry、sequence を検証する。
- immutable generations と冗長 state pointer による atomic cache を実装する。
- `CatalogStore::snapshot()` と `CatalogStore::install_verified()` を提供する。
- HTTP client は update command からのみ呼べる module boundary に置く。

#### Tests

- one-byte tamper
- wrong key / unknown key ID
- signature envelope tamper
- equal/lower sequence replay
- expired Catalog
- interrupted write
- corrupt active generation + same-sequence redundant state
- symlink/non-regular cache path
- concurrent readers during atomic replace
- bootstrap → sequence 2 → bootstrap rollback rejection

#### Gate

無署名・改ざん・rollback Catalog が resolver へ到達できない。

#### Rollback

runtime authority は legacy のまま。cache feature を無効化して revert できる。

---

### PR-03: ProviderDriver と probe の分離

#### 変更対象

```text
src/providers/mod.rs
src/providers/grok.rs
src/providers/cursor.rs
src/providers/codex.rs
src/providers/claude.rs
src/acp/session.rs
src/process/*
tests/config.rs
tests/provider_*audit.rs
```

#### 変更

- `VersionProbe` から `requirement` / `expected` を除去する。
- `IdentityProbe` と `ArtifactProbe` を導入する。
- provider module は exact version ではなく Driver ID/revision と launch contract を返す。
- Codex/Claude の composite identity を config self-attestation ではなく probe/covered metadata から取得する。
- legacy runtime pin を一時 adapter として残し、probe 結果に対して旧 constant を適用する。
- fixed args、environment allowlist、permission/process behavior は変更しない。

#### Tests

- Grok semver output
- Cursor opaque output
- Codex/Claude composite identity
- malformed/empty/multiple version output
- artifact digest reproducibility
- modified entrypoint/package metadata detection
- Driver contract snapshot

#### Gate

version の「観測」と「許可」が別 module になり、旧 behavior が全 test で維持される。

#### Rollback

legacy adapter が authority のため、probe abstraction を revert できる。

---

### PR-04: Resolver と shadow comparison

#### 新規候補

```text
src/compatibility/resolver.rs
src/compatibility/policy.rs
tests/provider_resolution.rs
```

#### 変更

- `verified` / `exact` / `experimental` policy model を実装する。
- deny-first blocked evaluation を実装する。
- target/Driver/identity/artifact の exact match を実装する。
- `ResolvedProviderLock` と canonical digest を実装する。
- runtime では旧 pin を authority のまま維持し、bootstrap resolver の結果と比較する shadow path を追加する。
- shadow mismatch は test では failure、開発 build では明示 diagnostics とする。

#### Tests

- verified exact match
- multiple supported versions
- recommended channel は acceptance と独立
- deprecated warning / not_after rejection
- blocked precedence
- exact entry mismatch
- hash mismatch
- target/Driver revision mismatch
- experimental unknown identity
- experimental known blocked rejection
- no automatic fallback

#### Gate

現在の4 provider の qualified artifact について、legacy pin と bootstrap resolver の allow/deny 結果が一致する。legacy では許可されていた未登録 local artifact は、期待された `artifact_not_cataloged` 差分として分類し、silent acceptance しない。

#### Rollback

shadow resolver を切り離し、legacy pin のみへ戻せる。

---

### PR-05: Config v2 と migration

#### 変更

- `ProviderProfileV2` と `CatalogConfig` を追加する。
- `version_policy` を default `verified` とする。
- v1 parse は migration command 専用に隔離する。
- `agentmux config migrate --check/--write` を追加する。
- exact match した旧 profile は Catalog entry pin へ変換する。
- docs example を integration test fixture にする。

#### Tests

- 4 provider すべての v1→v2 migration
- invalid old args
- hash/version に対応する entry 不在
- backup、atomic replace、mode `0600`
- unknown fields
- normal command で v1 config を拒否し migration instruction を表示
- generated v2 config の再parse

#### Gate

Catalog に qualified artifact として登録済みの有効 v1 profile を、意味を弱めず v2 exact profile へ移行できる。未登録 digest は自動承認されず、再導入または experimental への明示移行手順が表示される。

#### Rollback

v1 config の backup を使って v1 binary へ戻せる。v2 binary は normal runtime で silent legacy fallback を持たない。

---

### PR-06: Agent startup を Catalog resolver へ cutover

#### 変更対象

```text
src/runtime/*
src/acp/session.rs
src/model.rs
src/receipt.rs
src/error.rs
src/process/*
```

#### 変更

- Agent actor の startup phase に resolver を組み込む。
- process spawn 前に `ResolvedProviderLock` を確定する。
- profile/session fingerprint を lock digest ベースへ変更する。
- startup failure taxonomy に Catalog/identity/artifact/blocked error を追加する。
- `experimental` mutation double opt-in を enforcement する。
- Catalog snapshot は Agent initialization 中固定する。
- existing Agent followup は再resolveしない。
- legacy pin への runtime fallback を削除する。

#### Tests

- queued → resolve → running → terminal
- resolver failure が通常 receipt path へ流れる
- Catalog update 中の in-flight resolution
- Agent A は旧 lock、Agent B は新 lock
- provider path replacement 後も Agent A followup は同一 process/session
- launch 直前 file identity mismatch
- experimental mutation deny/double opt-in
- interrupt/shutdown/process cleanup regression

#### Gate

runtime の唯一の provider authorization path が Catalog resolver になる。legacy pin mismatch を fallback として使用しない。

#### Rollback

この PR 以降の rollback は binary rollback と config backup を使う。runtime 内の二重 authority は復活させない。

---

### PR-07: Persistence、Rust API、IPC v2

#### 変更

- `AgentSnapshot` に full lock を追加する。
- `RunReceipt` に lock summary を追加する。
- storage schema v2 migration を追加する。
- legacy record は `legacy_unknown` として保持する。
- Rust API export と provider snapshot を更新する。
- IPC protocol v2 と `reload_compatibility` operation を追加する。

#### Tests

- v1 DB fixture の upgrade
- lock round-trip
- legacy record を current Catalog から推測しない
- restart 後の receipt/list output
- redaction regression
- v1/v2 client-daemon mismatch
- reload concurrency
- stale client error taxonomy

#### Gate

新規 Agent/Run の provider identity が、process が終了した後も監査可能である。

#### Rollback

DB migration は旧 binary が読める範囲を事前に fixture で確認する。読めない場合は release note で DB backup/restore 手順を必須化し、downgrade を unsupported と明示する。

---

### PR-08: CLI、doctor、運用 observability

#### 変更

- compatibility status/update を追加する。
- file-based offline update を追加する。
- provider status/verify を追加する。
- doctor に Catalog/profile diagnostics を追加する。
- JSON output schema と exit code を文書化する。
- daemon 稼働中の verified reload を実装する。

#### Tests

- offline update
- invalid signature/rollback/expiry の exit code
- daemon online/offline update
- status JSON golden test
- provider verify による no-spawn 保証
- secret/redaction snapshot

#### Gate

ユーザーが spawn を試行せずに、Catalog と各 profile の利用可否を判断できる。

#### Rollback

runtime resolver は active Catalog を継続利用する。CLI update のみ無効化し、file copy + restart を緊急手順として残せる。

---

### PR-09: Qualification evidence と Catalog publish

#### 新規候補

```text
compatibility/evidence/README.md
scripts/render-provider-compatibility.*
.github/workflows/provider-candidate-discovery.yml
.github/workflows/provider-qualification.yml
.github/workflows/compatibility-publish.yml
tests/catalog_release.rs
```

#### 変更

- provider candidate discovery は issue/PR candidate 作成までとし、自動 publish しない。
- authenticated qualification は controlled runner で明示実行する。
- qualification result を machine-readable evidence JSON として保存する。
- evidence digest を Catalog entry に記録する。
- protected approval 後に full Catalog を署名し、immutable release asset と mutable discovery endpoint を発行する。
- published bytes、signature、digest、provenance を release artifact とする。
- `PROVIDER_COMPATIBILITY.md` は Catalog から生成または consistency test する。

#### Qualification evidence 必須項目

- provider identity / artifact digest
- target / OS version
- agentmux commit / binary digest
- Driver ID/revision
- suite version
- initialize/auth/session/new
- stream order / duplicate output
- 3-turn same-session continuity
- cancel recovery
- permission deny/allow behavior
- provider crash / malformed protocol
- process-tree cleanup
- capability inventory
- test start/end、result、reviewer approval reference

#### Gate

既存 Driver revision の新 provider version を、Cargo version や agentmux binary を変更せずに Catalog-only release できる。

#### Rollback

不良 entry は古い sequence を再配布せず、**より大きい sequence の corrective Catalog** で deprecated/blocked に変更する。

---

### PR-10: All-provider canary、docs、packaging

#### 変更

- まず1 provider で、現行 version と新 candidate version の2 entry を Catalog に収録する。
- Catalog-only update により、同じ agentmux binary が新 candidate を受理することを E2E で証明する。
- 続いて4 provider の current entry を qualification する。
- README、`PROVIDER_COMPATIBILITY.md`、provider setup、CLI reference、troubleshooting、upgrade policy、release procedure、architecture/status を更新する。
- Homebrew/package install 後の bootstrap/cache path と migration command を検証する。
- documentation sample を parser/CLI test へ接続する。

#### Gate

以下を実環境で証明する。

```text
agentmux binary A + Catalog sequence N     → provider version X accepted
agentmux binary A + Catalog sequence N + 1 → provider version Y accepted
```

binary A の hash、Cargo version、Driver revision は変化しない。

#### Rollback

Catalog sequence N+2 で Y を blocked、recommended channel を X へ戻す。既存 Y Agent は自動停止せず、新規 spawn のみ拒否する。

---

### PR-11: Cleanup と `2.0.0` release

#### 削除・整理

- runtime の `*_TESTED_VERSION` enforcement を削除する。
- `VersionProbe::expected()` と compile-time exact requirement を削除する。
- config の `validate_exact` と self-attested evidence fields を削除する。
- release manifest の Cargo-version coupling を削除する。
- legacy shadow comparison を削除する。
- historical manifest は `compatibility/legacy/` へ移す。
- dead code、temporary feature flag、migration-only internal adapter を整理する。
- `V2_DEFINITION_OF_DONE.md` を追加する。

#### Release gate

- format、clippy、all-target tests
- Catalog schema/signature/security suite
- config migration fixtures
- persistence/IPC upgrade fixtures
- all four authenticated conformance suites
- capability/permission/process audits
- race、chaos、lifecycle soak
- benchmark enforce
- release artifact、SBOM、provenance
- bootstrap Catalog/signature verification
- clean-install と v1-upgrade smoke test
- Catalog-only update canary

#### Gate

Definition of Done の全項目を満たし、runtime が compile-time provider version pin に依存しない。

---

## 15. Provider qualification と release policy

### 15.1 Catalog-only release でよい条件

すべて満たす必要がある。

- provider/target の既存 Driver ID/revision で起動できる。
- launch args が同一である。
- environment/auth contract が同一である。
- ACP stable v1 である。
- permission semantics が既存 contract と一致する。
- process supervision workaround の追加が不要である。
- identity/artifact probe の変更が不要である。
- authenticated conformance、permission、process、capability audit に合格する。
- agentmux requirement の範囲内である。

### 15.2 agentmux binary release が必要な条件

いずれかに該当する場合。

- launch command/args が変わる。
- environment allowlist、authentication、permission translation が変わる。
- version output/identity extractor を変更する必要がある。
- artifact verification 方法を変更する必要がある。
- process tree、cancel、shutdown workaround が必要になる。
- ACP protocol major/behavior contract が変わる。
- Driver capability contract が変わる。

この場合は Driver revision を上げ、新 agentmux binary と対応 Catalog entry を同時に release する。

### 15.3 supported version retention

- provider/target ごとに、原則として current recommended と直前2世代を保持する。
- security/critical regression がある version は即 `blocked` とし、世代数に関係なく受理しない。
- 古い version はまず `deprecated`、期限後に Catalog から除外または blocked とする。
- exact profile が参照する entry を削除する場合、release note と `doctor` warning を先行させる。

---

## 16. Test matrix

### 16.1 Unit

- Catalog strict parse/validation
- signature/keyring/digest
- sequence/expiry/state/channel
- identity normalization
- artifact digest
- policy evaluation
- lock canonicalization/digest
- config v1→v2 mapping
- error taxonomy

### 16.2 Integration

- bootstrap first-run
- signed remote/file update
- atomic cache/LKG
- daemon reload
- all profile policies
- exact entry reproducibility
- path/artifact replacement
- existing Agent immutability
- config migration and backup
- DB/IPC upgrade

### 16.3 Security

- unsigned/tampered Catalog
- replay/rollback Catalog
- unknown key / key rotation fixture
- malicious oversized input
- duplicate/ambiguous entries
- Catalog に command/args/env を注入できないこと
- symlink/non-regular config/cache/provider path
- artifact mismatch
- known blocked bypass attempt through experimental
- mutation double opt-in enforcement
- diagnostics redaction

### 16.4 Provider conformance

各 provider/version/target で以下を実施する。

- exact identity/artifact capture
- authentication discovery
- initialize stable v1
- session/new
- prompt/update ordering
- 3-turn followup continuity
- interrupt/cancel recovery
- permission deny/allow
- crash/malformed protocol
- no fallback
- process-tree cleanup
- output duplication
- capability inventory

### 16.5 Reliability/performance

- concurrent Catalog readers + update
- concurrent Agents resolving old/new Catalog snapshot
- lifecycle soak
- shutdown race
- process-tree audit
- current benchmark enforce
- digest/probe 追加による startup regression の計測

性能改善のために digest cache を追加する場合、canonical path + device/inode/size/mtime を key とし、security test を追加する。初期実装では correctness を優先し、安易な version-only cache を導入しない。

---

## 17. Rollout 手順

### Stage A: Version-behavior-preserving seed

- bootstrap Catalog に現在の4 exact identity と、qualification 済み supported artifact digest を収録する。
- qualified artifact では shadow resolver と legacy pin の一致を自動 test する。
- 未登録 v1 local checksum は自動承認せず、migration の blocking diagnostic として扱う。

### Stage B: Runtime cutover

- Catalog resolver を唯一の authority にする。
- まだ bootstrap と同じ entry だけを利用する。
- persistence/API/IPC/doctor を含む upgrade test を完了する。

### Stage C: Single-provider Catalog-only canary

- 既存 Driver のまま合格する新 version を1 provider だけ追加する。
- 同じ agentmux binary で Catalog update 前後の acceptance が変わることを実証する。
- existing Agent が旧 lock のまま継続することを確認する。

### Stage D: Four-provider operation

- 4 provider の qualification/publish 手順を一巡させる。
- recommended/deprecated/blocked lifecycle を少なくとも fixture または controlled canary で確認する。
- provider update が binary release queue を必要としない運用を確立する。

### Stage E: RC/GA

- clean install、v1 migration、air-gapped file update を実行する。
- release gate と Definition of Done を通す。
- legacy runtime pin と shadow code を削除して `2.0.0` を release する。

---

## 18. Rollback・インシデント手順

### 18.1 不良 provider entry

1. candidate entry を `blocked` にした higher-sequence Catalog を作る。
2. recommended channel を直前の verified entry へ戻す。
3. qualification evidence と incident reason を記録する。
4. Catalog を署名・publish する。
5. `doctor` と status で block reason を表示する。
6. 既存 Agent は自動停止しない。operator が必要に応じて interrupt/remove する。

### 18.2 不良 Catalog

- signature/schema validation で activation 前に拒否する。
- activation 後に論理不具合が判明した場合、古い sequence を再配布せず higher-sequence corrective Catalog を publish する。
- active が破損した場合のみ、同一またはより高い trust state の LKG を使用する。

### 18.3 signing key compromise

1. Catalog publish を停止する。
2. compromised key を keyring から除く agentmux security release を作る。
3. new key を埋め込み、必要なら dual-sign transition を行う。
4. highest accepted sequence を維持した corrective Catalog を新 key で発行する。
5. affected Catalog sequence と provider entry を advisory に記録する。

### 18.4 agentmux binary rollback

- config migration 前 backup と DB backup を使用する。
- IPC daemon/client を同一 major に揃える。
- storage schema が downgrade 非対応なら、旧 binary を同じ DB に直接接続しない。
- provider compatibility を取り戻す目的で、runtime legacy fallback を復活させない。

### 18.5 provider rollback

- versioned path に旧 provider artifact を保持する。
- exact profile の `catalog_entry` と executable/adapter path を旧 verified entry へ切り替える。
- running Agent はそのまま、new Agent だけ旧 entry を使用する。

---

## 19. 主なリスクと対策

| リスク | 影響 | 対策 |
|---|---|---|
| upstream version output の変更 | identity probe failure | Driver revision を上げ binary release。opaque parser fixture を保持 |
| 同一 version で artifact が複数 | false mismatch / supply-chain ambiguity | target/install artifact ごとに複数 exact entry。version-only acceptance は禁止 |
| adapter component version が観測不能 | identity が自己申告化 | digest に覆われた package metadata を Driver が読む |
| Catalog rollback/replay | blocked version の再許可 | monotonic sequence、highest-seen state、higher-sequence corrective release |
| Catalog key compromise | 任意 entry の信頼 | protected signer/KMS、embedded keyring、dual-sign rotation、incident release |
| Catalog expiry/offline | new Agent を起動できない | bootstrap、LKG、file update、十分な publication cadence。expired は fail closed |
| provider executable の更新 race | hash と起動物が不一致 | canonicalize、fd stat、digest、launch直前再検証、self-update disable |
| experimental の安全性低下 | 未検証 mutation | explicit profile、warning、known-block deny、mutation double opt-in |
| public API/IPC break | client incompatibility | major release、protocol v2、fixture/golden test、明確な mismatch error |
| docs と parser の乖離 | setup failure | 全 TOML/CLI example を automated test に含める |
| release pipeline の二重化 | 運用ミス | binary release checklist と Catalog release checklist を分離し、単一 evidence format を使用 |

---

## 20. Definition of Done

### Runtime

- [ ] `src/providers` の compile-time tested version が runtime authorization に使用されていない。
- [ ] `VersionProbe` は identity 観測だけを行う。
- [ ] Catalog resolver が唯一の provider authorization path である。
- [ ] `verified`、`exact`、`experimental` の契約が実装・文書化・テストされている。
- [ ] Agent ごとに exact `ResolvedProviderLock` が生成される。
- [ ] Catalog 更新で existing Agent の lock/process/session が変わらない。
- [ ] provider fallback、legacy pin fallback、network-on-spawn が存在しない。

### Trust / security

- [ ] Catalog が detached signature で検証される。
- [ ] tamper、unknown key、expiry、rollback、blocked、artifact mismatch が fail closed になる。
- [ ] Catalog は arbitrary command/args/environment を指定できない。
- [ ] cache update が atomic で LKG と highest sequence を維持する。
- [ ] experimental mutation が二重 opt-in である。

### Configuration / operations

- [ ] v1 profile を v2 exact profile に移行できる。
- [ ] migration は backup、mode `0600`、dry-run を備える。
- [ ] status/update/provider verify/doctor が JSON と人間向け出力を持つ。
- [ ] air-gapped signed file update が機能する。
- [ ] docs examples が parser/CLI tests を通る。

### Persistence / API

- [ ] AgentSnapshot と RunReceipt から provider identity と Catalog evidence を監査できる。
- [ ] v1 DB record を偽の current lock へ補完しない。
- [ ] IPC v2 mismatch が明示される。
- [ ] redaction、continuity、interrupt、process cleanup の既存契約が維持される。

### Release decoupling

- [ ] 同一 agentmux binary で、Catalog sequence の更新だけにより新 provider version を受理できる。
- [ ] Catalog-only release は Cargo version、Cargo.lock、binary hash、Driver revision を変更しない。
- [ ] Driver contract 変更時だけ binary release が必要になる。
- [ ] 4 provider の authenticated qualification と evidence publication が一巡している。
- [ ] binary release gate と Catalog release gate が別々に文書化・自動検証されている。

### Cleanup

- [ ] `validate_exact`、`*_TESTED_VERSION` runtime enforcement、shadow resolver が削除されている。
- [ ] 旧 compatibility manifest は historical data として隔離されている。
- [ ] README、setup、CLI、troubleshooting、upgrade、release、architecture/status が実装と一致している。
- [ ] `2.0.0` clean install、v1 upgrade、Catalog-only canary が合格している。

---

## 21. 最初に着手する順序

実装開始時は、以下の順番を崩さない。

1. ADR で trust boundary と schema を固定する。
2. 現在の exact 4-version 集合を bootstrap Catalog の identity seed に写し、supported artifact digest は qualification で収集する。
3. runtime に接続する前に Catalog parser/signature/cache を完成させる。
4. provider probe を permission/process contract から切り離さず、version authorization だけ分離する。
5. legacy pin と bootstrap resolver の shadow parity を証明する。
6. config migration を先に提供してから runtime authority を切り替える。
7. cutover 後は legacy fallback を置かない。
8. lock を persistence/API/IPC に通してから Catalog-only canary を行う。
9. canary 合格後に publish workflow と docs を最終化する。
10. 最後に compile-time pin enforcement と temporary migration code を削除する。

この順序により、初期フェーズでは許可集合を広げず、artifact trust はむしろ qualification 済み集合へ狭める。署名・resolver・config・runtime の各境界を個別に検証してから provider 更新を agentmux release から切り離せる。

---

## 22. 現行リポジトリ上の主な変更対象

```text
Cargo.toml
src/config.rs
src/doctor.rs
src/error.rs
src/ipc.rs
src/lib.rs
src/main.rs
src/model.rs
src/receipt.rs
src/security.rs
src/storage.rs
src/acp/session.rs
src/process/*
src/providers/mod.rs
src/providers/grok.rs
src/providers/cursor.rs
src/providers/codex.rs
src/providers/claude.rs
src/runtime/*
tests/config.rs
tests/contract.rs
tests/ipc.rs
tests/persistence.rs
tests/provider_capability_audit.rs
tests/provider_conformance.rs
tests/provider_fault_conformance.rs
tests/provider_permission_audit.rs
tests/provider_process_audit.rs
tests/release_manifest.rs
tests/process_tree.rs
tests/lifecycle_soak.rs
.github/workflows/ci.yml
.github/workflows/release.yml
compatibility/*
PROVIDER_COMPATIBILITY.md
CONFORMANCE.md
PRODUCT_CONTRACT.md
NON_GOALS.md
SECURITY.md
README.md
docs/provider-setup.md
docs/cli-reference.md
docs/troubleshooting.md
docs/upgrade-policy.md
docs/release.md
docs/architecture.ja.md
docs/status.ja.md
```

新規 module/file 名は実装時に repository convention に合わせて調整してよい。ただし、Driver、Catalog、resolver、cache/signature、config migration の責務を再び単一 module に混在させない。

---

## 23. 現状確認に使用した repository sources

- `README.md`
- `compatibility/agentmux-1.0.0.json`
- `src/config.rs`
- `src/providers/mod.rs`
- `src/providers/{grok,cursor,codex,claude}.rs`
- `src/acp/session.rs`
- `src/model.rs`
- `src/storage.rs`
- `src/main.rs`
- `docs/provider-setup.md`
- `docs/upgrade-policy.md`
- `docs/release.md`
- `docs/architecture.ja.md`
- `CONFORMANCE.md`
- `NON_GOALS.md`
- `.github/workflows/{ci,release}.yml`
- `tests/provider_*`, `tests/release_manifest.rs`

Repository: `https://github.com/coconiiruzo/acpxx`
