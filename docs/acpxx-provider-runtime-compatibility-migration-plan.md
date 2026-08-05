# acpxx Provider Compatibility Catalog 撤去・実行時互換性モデル移行計画

- **Status:** Implemented (v2.0.0 release gates passed on 2026-08-05)
- **作成日:** 2026-08-05
- **対象:** `main`（PR #4 merge commit `9a9a982` 以降）
- **移行先リリース:** `v2.0.0` 初回公開前
- **実施方式:** 1つの移行PR内で `revert → forward rebuild`
- **重要:** revertコミット単体はmainへマージしない

実装記録: 機械的revertはcommit `a8c41bc`、77 pathの仕分けは
[`migration/pr4-salvage-inventory.md`](migration/pr4-salvage-inventory.md)、最終設計判断は
[ADR-0006](adr/0006-runtime-negotiated-provider-compatibility.md)を参照する。

---

## 1. 移行の目的

次の二つを、製品契約・実装・運用のすべてで成立させる。

1. ユーザーは、acpxxの更新や中央データの公開を待たず、未確認を含む新しいprovider versionを標準経路で起動できる。
2. acpxxメンテナは、providerの新versionを継続的に発見・検証・署名・公開する責務を負わない。

この移行では、PR #4が導入したsigned Compatibility Catalogを実行認可から完全に撤去する。ただし、PR #4に含まれる有用な責務分離、実行ファイル観測、artifact差し替え検知、監査情報、設定移行、セッション・プロセス管理の改善は必要な範囲で再導入する。

### 完成後の保証

acpxxは、以下を満たすproviderを、中央で認定されていないversionであることだけを理由に拒否しない。

- 対象provider用の組み込み`ProviderDriver`で起動できる
- 実行ファイルのローカル安全性検査を通過する
- ユーザーが設定したローカルassertionがある場合は、それに一致する
- ACP `initialize`で対応protocol versionに合意できる
- 実行する操作に必要なACP capabilityを備える
- 通常のpermission policyを通過する

### 保証しないこと

次は保証しない。

- providerが起動command、固定引数、認証方式、ACP protocol、permission semanticsを破壊的変更しても、acpxx更新なしで動作すること
- 未確認versionの完全な動作互換性
- providerの自動install・自動update
- providerの供給元やbinary自体の信頼性
- 任意providerを追加できるplugin system
- providerごとのversion追跡、サポート期限、脆弱version blocklistの中央運用

破壊的変更により既存`ProviderDriver`の契約が成立しなくなった場合だけ、通常のdriver修正としてacpxxを更新する。

---

## 2. 固定する設計判断

この移行中に再検討しない判断を以下に固定する。

| 項目 | 最終判断 |
|---|---|
| 対応provider | Codex / Claude / Grok / Cursorの閉じた4種類を維持 |
| transport | stable ACP v1 over stdioを維持 |
| ACP v2 | 本移行の対象外 |
| 中央Compatibility Catalog | 廃止 |
| Catalog署名・sequence・expiry | 廃止 |
| 未知version | 標準で起動を試みる |
| version probe | 観測・診断専用。標準では失敗しても起動を拒否しない |
| artifact SHA-256 | 観測証跡。ローカルassertionがある場合だけ認可条件 |
| executable安全性検査 | 常時必須 |
| artifact差し替え検知 | 常時必須 |
| ACP initialize | 実行時互換性の必須gate |
| ACP capability | 操作単位の必須gate |
| mutation権限 | provider versionと分離し、既存permission policyだけで判定 |
| provider更新 | acpxxは実施しない |
| 確認済みversion情報 | 人間向け参考情報。runtimeは参照しない |
| 移行方式 | PR #4を機械的にrevert後、必要な機能だけforward-port |
| 公開API | Catalog由来の型・引数・エラーを削除 |
| config | 最終版schema v2へ再定義 |
| SQLite | schema v3へ移行 |
| IPC | 未公開のため最終版IPC v2として整理し、Catalog-era shapeは保証しない |

---

## 3. 最終runtimeフロー

```text
profile / SpawnRequest
  ↓
組み込みProviderDriverを選択
  ↓
実行ファイルをcanonicalize
  ↓
必須のローカル安全性検査
  - regular file
  - current user または root 所有
  - group/world writableではない
  - launch artifactはexecutable
  ↓
launch artifactのSHA-256とfile identityを観測
  ↓
version/component probeをbest effortで実行
  - 成功: 観測情報へ記録
  - 失敗/未知形式: warningとして記録
  ↓
任意のローカルassertionを評価
  - assertionなし: 続行
  - assertion一致: 続行
  - assertion不一致/検証不能: terminal receiptで失敗
  ↓
spawn直前にfile identityを再検査（TOCTOU防止）
  ↓
provider processをspawn
  ↓
ACP initialize
  - protocol version negotiation
  - agentInfoを記録
  - capabilitiesを記録
  ↓
protocol不一致なら失敗
  ↓
必要なcapabilityがなければ、その操作を明示的に失敗
  ↓
session/new → prompt → followup
  ↓
実行時identityをAgent / Run / receiptへ記録
```

### 実行可否のgate

| 検査 | assertionなし | assertionあり |
|---|---:|---:|
| executableが解決できない | 拒否 | 拒否 |
| file ownership/mode/type不正 | 拒否 | 拒否 |
| launch artifactがspawn前に変化 | 拒否 | 拒否 |
| version probe失敗 | 続行 | version assertionがあれば拒否 |
| component metadata取得失敗 | 続行 |該当component assertionがあれば拒否 |
| SHA-256が未確認version | 続行 | configured digestと違えば拒否 |
| Catalog未掲載 | Catalog自体が存在しない | Catalog自体が存在しない |
| ACP initialize不成立 | 拒否 | 拒否 |
| ACP protocol不一致 | 拒否 | 拒否 |
| 必要capabilityなし | 該当操作を拒否 | 該当操作を拒否 |
| mutation要求 | permission policyで判定 | permission policyで判定 |

---

## 4. 最終データモデル

### 4.1 残す型と責務

`ProviderDriver`は維持する。以下は引き続きcompiled behaviorとする。

- provider ID
- driver ID / revision
- launch command
- 固定引数
- environment allowlist
- fixed environment
- 認証方式の選択
- startup timeout
- stable ACP v1
- brokerが必要とするcapability
- provider固有のprocess workaround
- best-effort identity/component probe
- artifact観測方法

providerの新versionそのものはdriverへ記載しない。

### 4.2 `ResolvedProviderLock`の置換

`ResolvedProviderLock`、`CompatibilityLevel`、`VersionPolicy`を削除し、認可を意味しない`ProviderExecutionIdentity`へ置換する。

概念形:

```rust
pub struct ProviderExecutionIdentity {
    pub provider: ProviderId,
    pub driver_id: DriverId,
    pub driver_revision: u32,
    pub target: String,

    pub executable_path: PathBuf,
    pub launch_sha256: String,
    pub observed_version: ProbeObservation<String>,
    pub observed_components: BTreeMap<String, ProbeObservation<String>>,

    pub acp_protocol_version: Option<u32>,
    pub acp_agent_info: Option<ImplementationInfo>,
    pub capability_digest: Option<String>,

    pub assertion_result: AssertionResult,
}
```

`ProbeObservation<T>`は最低限、次を区別する。

```text
observed(value)
unavailable(reason)
malformed(raw_or_reason)
```

注意:

- `unavailable`と`malformed`は、assertionがない限りfailureではない。
- version文字列は互換性の推論に使わない。
- semver range判定は導入しない。
- `ProviderExecutionIdentity`は監査・診断・continuity用であり、「verified」「trusted」「qualified」を表さない。

### 4.3 artifact関連の名称変更

Catalog由来の語彙を消す。

| 現在 | 移行後 |
|---|---|
| `QualifiedArtifactFile` | `ObservedArtifactFile` |
| `verify_qualified_files()` | `verify_unchanged()` |
| `ProviderLockSummary` | `ProviderIdentitySummary` |
| `provider_lock` | `provider_identity` |
| `provider_profile_fingerprint`の入力 | Catalog lock digestではなく、driver・profile・observed launch artifactから生成 |

continuity fingerprintには、少なくとも次を含める。

- provider ID
- driver ID / revision
- canonical executable path
- launch artifact SHA-256
- driver-owned launch args
- permissionに影響するprofile設定

Catalog entry、sequence、expiry、署名、qualification evidenceは含めない。

### 4.4 ローカルassertion

初期実装では次だけを提供する。

```rust
pub struct ProviderAssertions {
    pub version: Option<String>,
    pub components: BTreeMap<String, String>,
    pub launch_sha256: Option<String>,
}
```

規則:

- 全項目exact match
- version range、channel、recommended、latest指定は持たない
- 空のassertionが標準
- assertionはユーザー所有のローカル制約であり、acpxxメンテナは値を配布・更新しない
- version assertionはdriverが正規化したdisplay versionへ適用
- component assertionは、Codex adapter内のCodex versionやClaude SDK versionなど、driverが観測できたcomponentへ適用
- `launch_sha256`は実際にspawnするcanonical launch artifactへ適用

---

## 5. 最終config schema

Catalog-era schema v2は未公開の過渡状態として扱い、最終版schema v2を次の形へ再定義する。

### Grok / Cursor

```toml
schema_version = 2

[profiles.grok-default]
provider = "grok"
executable = "/absolute/path/to/grok"
permissions = "deny"

# 任意。省略時は未知versionを含めて起動を試みる。
[profiles.grok-default.assertions]
version = "0.2.118"
launch_sha256 = "0123456789abcdef..."
```

### Codex / Claude adapter

```toml
schema_version = 2

[profiles.codex-default]
provider = "codex"
adapter_path = "/absolute/path/to/codex-acp"
permissions = "deny"

# 任意
[profiles.codex-default.assertions]
version = "1.1.9"
launch_sha256 = "0123456789abcdef..."

[profiles.codex-default.assertions.components]
codex = "0.145.0"
```

### 削除するconfig field

- top-level `catalog`
- `version_policy`
- `catalog_entry`
- Catalog path / signature path / source
- qualification evidence
- Catalog由来のmutation opt-in

### 維持する安全性

- config fileはregular file
- mode `0600`
- 1 MiB上限
- absolute executable path
- atomic replacement
- migration前backup
- driver-owned args / envをユーザーconfigへ戻さない

---

## 6. repository差分の扱い

PR #4は77ファイルにまたがるため、全差分を「残す・書き換える・削除する」に分類してから作業する。

### 6.1 残す／再導入するもの

- `ProviderDriver`とprovider identity observationの分離
- driver revision
- executable ownership / mode / type検査
- SHA-256観測
- inode/device/mtime等によるartifact差し替え検知
- spawn直前のTOCTOU再検査
- process ownership / cleanup強化
- ACP session lifecycleの修正
- permission hostの安全性
- bounded stderr / event handling
- immutable per-Agent process/session identity
- receiptとSQLiteの監査情報
- config migrationのsecure read、backup、atomic replace
- IPC / CLI / doctorのCatalog非依存改善
- conformance、fault、permission、process lifecycle test
- release reproducibility・benchmarkのCatalog非依存部分

### 6.2 書き換える主要ファイル

- `PRODUCT_CONTRACT.md`
- `README.md`
- `NON_GOALS.md`
- `V2_DEFINITION_OF_DONE.md`
- `CONFORMANCE.md`
- `PROVIDER_COMPATIBILITY.md` または後継`TESTED_PROVIDERS.md`
- `docs/architecture.ja.md`
- `docs/cli-reference.md`
- `docs/installation.md`
- `docs/provider-setup.md`
- `docs/release.md`
- `docs/rust-api.md`
- `docs/status.ja.md`
- `docs/troubleshooting.md`
- `docs/upgrade-policy.md`
- `src/providers/*`
- `src/acp/session.rs`
- `src/api.rs`
- `src/config.rs`
- `src/doctor.rs`
- `src/ipc.rs`
- `src/lib.rs`
- `src/main.rs`
- `src/model.rs`
- `src/receipt.rs`
- `src/runtime/agent_actor.rs`
- `src/runtime/registry.rs`
- `src/storage.rs`
- Catalogに結合したintegration test

### 6.3 削除するもの

#### runtime

- `src/compatibility/bootstrap.rs`
- `src/compatibility/catalog.rs`
- `src/compatibility/resolver.rs`
- `src/compatibility/signature.rs`
- `src/compatibility/store.rs`
- Catalogだけを表すmodel
- `VerifiedCatalog`
- `CatalogStore`
- `CompatibilityCatalog`
- `CatalogEntry`
- `CatalogSignatureEnvelope`
- `VersionPolicy`
- `CompatibilityLevel`
- Catalog status / update / reload API

genericな`DriverId`、observed identity、artifact digestが同moduleにある場合は、`providers`または新しい`provider_identity` moduleへ移してから`src/compatibility`全体を削除する。

#### repository data

- `compatibility/bootstrap/**`
- `compatibility/evidence/**`
- `compatibility/legacy/**`
- `compatibility/schema/**`

Git履歴にPR #4が残るため、obsolete Catalog dataをmainへ保存し続けない。

#### GitHub Actions

- `.github/workflows/compatibility-publish.yml`
- `.github/workflows/provider-candidate-discovery.yml`
- `.github/workflows/provider-qualification.yml`

provider conformance test自体は残すが、version発見・認定・公開workflowには接続しない。

#### scripts

- `scripts/record-provider-qualification.py`
- `scripts/render-provider-compatibility.py`
- `scripts/sign-provider-catalog.sh`
- `scripts/package-release.sh`内のCatalog asset処理

#### tests

削除または置換:

- `tests/catalog_release.rs`
- `tests/catalog_runtime.rs`
- `tests/compatibility_catalog.rs`
- Catalog sequence / expiry / signature / rollback / channelのtest

#### dependencies

利用箇所を`rg`と`cargo tree`で確認した上で、Catalog専用なら削除する。

- `base64`
- `ed25519-dalek`
- `reqwest`
- `time`

`semver`はversion observationやpackage version処理に必要なら残す。互換性range判定には使わない。

---

## 7. 実施戦略

### 7.1 1つのPRで完結させる

以下を別PRに分けない。

- PR #4のrevert
- exact-version authorizationの撤去
- runtime identityの再導入
- config migration
- SQLite migration
- CLI / IPC変更
- Catalog artifact・workflow削除
- documentation
- tests

revertだけがmainへ入ると、PR #4以前のcompile-time exact pinへ戻り、要件を満たさないためである。

### 7.2 履歴は書き換えない

```bash
git switch main
git pull --ff-only
git switch -c refactor/runtime-provider-compatibility

# 任意のローカル安全参照。公開tagは不要。
git branch backup/catalog-pr4 9a9a982

# 差分仕分け用
git diff --name-status daa3626..4e5c964 \
  > /tmp/acpxx-pr4-files.txt
git diff --stat daa3626..4e5c964

# 機械的revert
git revert -m 1 9a9a982
```

revert時点でtestを通し、「機械的revert」と後続の設計変更を混ぜない。

---

## 8. フェーズ別実行計画

## Phase 0 — 契約固定とsalvage inventory

### MIG-000: baselineを固定する

実施:

- 作業開始時の`main` commitを記録
- merge commit `9a9a982`と親`daa3626`を再確認
- PR #4以降に別commitがある場合は、その差分を別枠で保護
- 現行のfull test結果を保存
- current config / SQLite fixtureを保存

完了条件:

- 比較対象commitが明示されている
- revertで巻き戻してはいけないPR #4以外の変更が特定されている
- Catalog-era configとDBのmigration fixtureがrepository内test fixtureとして保存されている

### MIG-001: 77ファイルを分類する

各ファイルを次に分類したmanifestを作る。

```text
KEEP_AS_IS
FORWARD_PORT
REWRITE
DELETE
```

特に`src/acp/session.rs`、`src/api.rs`、`src/config.rs`、`src/storage.rs`は、Catalog変更と一般的hardeningが混在するため、行単位で分類する。

完了条件:

- PR #4の全77ファイルに分類がある
- `FORWARD_PORT`対象には対応testが紐づいている
- 「理由なしに残す」項目がない

---

## Phase 1 — 機械的revert

### MIG-100: PR #4をrevertする

実施:

```bash
git revert -m 1 9a9a982
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

完了条件:

- revert commitに手修正を混ぜていない
- PR #4以前のbaselineとしてbuild/testが成立
- revert commit単体はPR内だけに存在し、mainへ単独mergeされない

### MIG-101: revertで失われた非Catalog改善を一覧化する

次を優先してforward-port対象へ確定する。

- process tree cleanup
- ACP lifecycle / timeout / error classification
- permission audit
- bounded event/log handling
- artifact file safety
- SQLite / IPCの一般的hardening
- config secure migration
- reproducible packaging

完了条件:

- 各項目について「再導入commit」と「回帰test」が決まっている

---

## Phase 2 — provider observation core

### MIG-200: generic identity型をCatalogから分離する

実施:

- `DriverId`
- observed version/component
- artifact digest
- executable file identity
- execution identity summary

を`src/providers`または`src/provider_identity.rs`へ移す。

削除する語彙:

- verified
- qualified
- catalog entry
- compatibility level
- resolution
- authorization lock

完了条件:

- provider identity型がCatalog typeへ依存しない
- runtime moduleから`compatibility` moduleをimportしない
- public JSONにCatalog語彙がない

### MIG-201: version probeをbest effort化する

実施:

- probe timeoutとoutput上限を維持
- probe成功時はnormalized valueとraw/display valueを記録
- probe失敗、非zero exit、未知形式、metadata欠落を`ProbeObservation`へ変換
- assertionが空ならprobe failureをterminal failureにしない
- providerごとのhard-coded tested versionを削除
- package metadata layoutが変わっても標準起動を止めない

完了条件:

- fixtureが`9999.0.0`を返しても起動前認可を通る
- version commandが失敗してもassertionなしではspawnへ進む
- Cursor等の非semver文字列を記録できる
- Codex / Claudeのpackage metadataが見つからなくてもassertionなしではspawnへ進む

### MIG-202: mandatory artifact safetyを再導入する

実施:

- canonical path
- regular file
- owner
- writable bit
- executable bit
- launch SHA-256
- file identity
- spawn直前再検査

を維持する。

supplementary package metadataはbest effort観測とし、launch executable検査とは分離する。

完了条件:

- launch artifact差し替えは`provider_artifact_changed`
- package metadata欠落だけでは拒否しない
- world/group writable executableは常に拒否
- symlink解決後の実体を観測・起動する

### MIG-203: local assertion evaluatorを実装する

実施:

- `ProviderAssertions::default()`は無制約
- version exact match
- component exact match
- launch SHA-256 exact match
- mismatchとunverifiableを構造化して返す
- assertion failureを単一のpublic failure codeへ正規化

推奨failure code:

```text
provider_assertion_failed
```

詳細理由はmessageまたはstructured detailで区別する。

完了条件:

- assertionなしは未知versionを許可
- version mismatchはprocess spawn前に失敗
- digest mismatchはprocess spawn前に失敗
- version assertion設定時にprobe不能なら明示的に失敗
- assertion値はreceiptへ秘密情報として扱わず、結果だけを記録

---

## Phase 3 — runtime / ACP session移行

### MIG-300: BrokerからCatalog stateを除去する

削除:

- `catalog: Arc<RwLock<...>>`
- `catalog_store`
- bootstrap Catalog
- Catalog付きconstructor
- compatibility status / reload
- Catalog snapshotをAgent actorへ渡す処理
- listのrecommended entry / sequence / digest生成

完了条件:

- `Broker::new`とSQLite constructorがCatalogを構築しない
- `serve`と`spawn`にCatalog path/cache/keyringが不要
- broker起動時にCatalog関連I/Oがない

### MIG-301: session startupをruntime negotiationへ置換する

現在の流れ:

```text
observe → Catalog resolve → experimental mutation gate → spawn → initialize
```

移行後:

```text
observe → local assertion → TOCTOU recheck → spawn → initialize → capability check
```

実施:

- `AcpSessionSetup`から`version_policy`、`catalog_entry`、`allow_unverified_mutations`、`catalog`を削除
- `resolve_provider()`呼び出しを削除
- `UnverifiedMutationDenied`分岐を削除
- permission policyは全versionで同一に適用
- pre-spawn identityをeventへ通知
- initialize後にACP protocol、agentInfo、capabilitiesでidentityを補完
- process spawn後の失敗は通常のRun terminal receiptへ流す

完了条件:

- unknown versionが追加flagなしでACP initializeまで到達
- mutation権限の判定にversion情報を参照しない
- protocol mismatchは`acp_initialize_failed`
- optional capability欠落は、そのfeatureを使う時点で明示的に拒否
- baseline text promptに必要なcontractが成立しなければ明示的に失敗

### MIG-302: continuity fingerprintを再定義する

実施:

- `ResolvedProviderLock::canonical_digest()`を廃止
- provider/profile launch fingerprint用のcanonical serializerを追加
- BTreeMap、artifact subject sort等で決定的に生成
- Catalog fieldsは入力しない
- Agent lifetime中はfingerprintを固定
- provider executable更新後に新規Agentを作ると、新identityを観測
- 既存Agentは起動済みprocess/sessionを継続し、再解決しない

完了条件:

- same Agent followupのcontinuityが維持される
- process/sessionの差し替えはcontinuity loss
- disk上のprovider更新が既存processを暗黙に切り替えない
- 新規Agentだけが更新後artifactを使用する

### MIG-303: identityをsnapshot / receiptへ伝播する

実施:

- `AgentSnapshot.provider_lock` → `provider_identity`
- `RunSnapshot.provider_lock` → `provider_identity`
- `RunReceipt.provider_lock` → `provider_identity`
- `AcpSessionEvent::Resolved`を、認可を意味しない名称へ変更
- startup途中で失敗した場合も、取得済みpre-spawn identityをreceiptへ残す
- capabilitiesはbounded/canonicalなsummaryまたはdigestで保存
- provider-private metadataやsecretは保存しない

完了条件:

- 成功receiptにversion、components、path、launch digest、driver revision、ACP protocol、agentInfo、capability summaryがある
- version不明でもreceiptを生成できる
- Catalog entry / sequence / digest / compatibility levelがreceiptにない

---

## Phase 4 — public API / IPC / CLI

### MIG-400: Rust APIを整理する

`SpawnRequest`から削除:

- `version_policy`
- `catalog_entry`
- `allow_unverified_mutations`

追加:

```rust
pub assertions: ProviderAssertions
```

defaultは空。

`ProviderSnapshot`から削除:

- recommended Catalog entry
- Catalog sequence
- Catalog digest

維持:

- provider ID
- protocol policy
- driver ID / revision
- required capabilities

完了条件:

- API利用者がCatalog typeをimportする必要がない
- empty assertionsの通常spawnが最短経路
- `cargo doc`上にCatalog前提の説明がない

### MIG-401: failure taxonomyを整理する

削除:

- `catalog_unavailable`
- `provider_not_verified`
- `provider_blocked`
- `unverified_mutation_denied`

追加または維持:

- `provider_spawn_failed`
- `provider_assertion_failed`
- `provider_artifact_changed`
- `acp_initialize_failed`
- `authentication_failed`
- `session_create_failed`
- `protocol_corruption`
- `provider_crashed`
- `continuity_lost`

完了条件:

- 未知versionだけを理由にしたfailure codeが存在しない
- assertion failureとACP incompatibilityが区別できる

### MIG-402: IPC v2の最終shapeを確定する

実施:

- Catalog update/status/reload requestを削除
- Catalog fieldsをrequest/responseから削除
- `provider_lock`を`provider_identity`へ変更
- Catalog-era IPC v2との互換性は保証しない
- persisted JSON読込だけ、必要なら`serde(alias = "provider_lock")`等の一時互換を持つ
- outputは常に最終名称だけを返す

完了条件:

- IPC schema fixtureにCatalog語彙がない
- client / daemon mismatchが明示的に検出される
- Catalog commandを送るrouteがない

### MIG-403: CLIを整理する

削除:

```text
agentmux compatibility status
agentmux compatibility update
--allow-unverified-mutations
--version-policy
--catalog-entry
Catalog source/path/signature option
```

置換:

```text
agentmux provider inspect PROFILE --json
```

`provider inspect`の出力:

- resolved provider/driver
- canonical executable path
- executable safety結果
- observed version/components
- launch SHA-256
- local assertion結果
- probe warning

必要なら`--connect`でACP initializeまで確認し、protocol/agentInfo/capabilitiesを表示する。標準inspectはprovider sessionを開始しない。

`doctor`は次を報告する。

- config validity
- executable availability/safety
- probe observation
- assertion result
- optional ACP connection check
- Catalog statusは報告しない

完了条件:

- CLI helpにCatalog用語がない
- latest providerを使うためのescape hatchが不要
- inspect結果は「verified」ではなく「observed」「assertion matched」と表現する

---

## Phase 5 — config migration

### MIG-500: 最終schema v2 parserを実装する

実施:

- top-level `catalog`を削除
- profileへoptional `assertions`を追加
- provider別path fieldとpermissionsを維持
- strict `deny_unknown_fields`を維持
- default configはassertionなし
- schema v2内の過渡shapeを事前判定する

完了条件:

- 新規configは中央データなしでload可能
- unknown provider versionを標準で許可
- malformed assertionはconfig validationで拒否

### MIG-501: v1 config migrationを更新する

旧v1の明示的なversion / digestは、ユーザーが設定した制約としてlocal assertionsへ保存する。

mapping:

- Grok / Cursor `version` → `assertions.version`
- Grok / Cursor `sha256` → `assertions.launch_sha256`
- Codex `adapter_version` → `assertions.version`
- Codex `bundled_codex_version` → `assertions.components.codex`
- Claude `adapter_version` → `assertions.version`
- Claude `claude_agent_sdk_version` → `assertions.components.claude_agent_sdk`
- 既存path / permissions → 維持
- args / authentication / self-attested evidence → 削除しwarning
- environment lock → 削除しwarning

migration outputには次を明記する。

```text
このprofileは旧pinをlocal assertionとして保持したため、引き続き固定されています。
providerの新versionを自動的に試すにはassertions tableを削除してください。
```

完了条件:

- v1 pinを黙って失わない
- migration後configはCatalogを参照しない
- `--check`だけでは書き換えない
- `--write`はbackup後にatomic replace

### MIG-502: Catalog-era v2 config migrationを追加する

schema versionが2でも、次があれば過渡Catalog shapeと判定する。

- top-level `catalog`
- `version_policy`
- `catalog_entry`

mapping:

- `verified` → assertionsなし
- `experimental` → assertionsなし
- `exact` → 指定entryを旧Catalog dataから解決し、version/components/launch digestをlocal assertionsへ変換
- permissions/path → 維持
- Catalog source/path/signature → 削除
- mutation double opt-in → 削除

`exact` entryを解決できない場合は、制約を黙って落とさずmigrationをblockする。migration-only legacy parserは残してよいが、signature検証・network update・runtime Catalog storeは残さない。

完了条件:

- Catalog-era default profileはfloating profileへ変換
- exact profileはlocal assertionへ保存
- migration中にnetworkへ接続しない
- active Catalogの署名やsequenceを新configへ持ち込まない

### MIG-503: backup命名を一般化する

現行の`.v1-backup-*`から、移行元を限定しない名称へ変更する。

例:

```text
providers.toml.pre-runtime-compat-<unix-time>.bak
```

完了条件:

- v1とCatalog-era v2の両方でbackupが作られる
- backup modeは`0600`
- write途中の失敗で元fileが壊れない

---

## Phase 6 — SQLite persistence migration

### MIG-600: schema v3を定義する

最終column:

```text
agents.provider_identity_json
runs.provider_identity_json
```

`provider_lock_json`は最終schemaから削除する。

実施方式:

- transaction開始
- v3 tableを作成
- v2 rowsを読み込んで変換
- v3 tableへinsert
- old tableを置換
- schema versionを3へ更新
- commit

完了条件:

- v1 DB → v3
- Catalog-era v2 DB → v3
- 新規DB → v3
- migration失敗時はtransaction rollback

### MIG-601: old lockをobserved identityへ変換する

保存する:

- provider
- driver ID / revision
- target
- observed version/components
- artifact digests

破棄する:

- compatibility level
- catalog entry ID
- catalog sequence
- catalog digest
- qualification state

ACP protocol / agentInfo / capabilityが旧rowにない場合は`None`にする。履歴を捏造しない。

完了条件:

- 旧terminal receiptを読み出せる
- Catalogによる認可状態を新identityへ誤変換しない
- migration後のruntimeがold lockを実行許可に使わない

### MIG-602: JSON shape migrationを実装する

実施:

- snapshot/receipt内の`provider_lock`を`provider_identity`へ変換
- migration readerは旧fieldを読める
- normal serializerは新fieldだけを書く
- metadata redaction policyを再確認
- session ID、prompt、assistant output、secretは従来どおり保存しない

完了条件:

- v2 fixtureの全rowをrestoreできる
- host restart reconciliationが維持される
- terminal receiptのcompletion sequenceを維持
- active process/sessionをrestart後に復元したと偽らない

---

## Phase 7 — Catalog infrastructureの完全撤去

### MIG-700: source / asset / workflowを削除する

Section 6.3の対象を削除する。

完了条件:

```bash
rg -n \
  'CompatibilityCatalog|VerifiedCatalog|CatalogStore|CatalogEntry|VersionPolicy|CompatibilityLevel|catalog_entry|catalog_sequence|allow_unverified_mutations|provider_not_verified|compatibility update' \
  . \
  --glob '!docs/adr/0005-provider-compatibility-catalog.md' \
  --glob '!tests/fixtures/**'
```

が、migration-only legacy parserとhistorical ADR以外で0件。

### MIG-701: dependencyを削減する

実施:

```bash
cargo tree -i base64
cargo tree -i ed25519-dalek
cargo tree -i reqwest
cargo tree -i time
```

Catalog以外の利用がなければ`Cargo.toml`と`Cargo.lock`から削除する。

完了条件:

- runtime binaryにCatalog download/signature dependencyがない
- `serve` / `spawn` / `inspect`がacpxx自身のnetwork requestを行わない
- release artifactにCatalog JSON / signatureを同梱しない

### MIG-702: packagingを整理する

実施:

- release manifestからCatalog assetを削除
- Homebrew templateからCatalog cache/setupを削除
- package reproducibility testを維持
- binary + license + docs等、必要なartifactだけを含める

完了条件:

- clean machineでCatalogなしにinstall/build/startできる
- packaging testがCatalog fileの存在を要求しない

---

## Phase 8 — documentationと確認済みversion情報

### MIG-800: ADRを更新する

- `docs/adr/0005-provider-compatibility-catalog.md`を`Superseded`にする
- 新規ADRを追加する

推奨名:

```text
docs/adr/0006-runtime-negotiated-provider-compatibility.md
```

ADRに記載する。

- 中央Catalogを採用しない理由
- version observationとauthorizationの分離
- ACP initialization/capability negotiationを互換性gateにする理由
- local assertionの位置づけ
- security checksを維持する理由
- breaking provider changeだけがdriver updateを要求すること
- 運用上の責任境界

完了条件:

- Catalog設計が現行判断として読めない
- 判断の履歴は失われていない

### MIG-801: Product Contractを書き換える

最低限、次をMUST / MUST NOTで固定する。

```text
MUST:
- unknown provider versionを、その事実だけで拒否しない
- ACP initializeとcapabilityを実行時に確認する
- observed identityをreceiptへ残す
- local assertionsをexactに評価する
- permission policyをversionから独立させる
- provider artifact差し替えをspawn前に検知する

MUST NOT:
- central Catalogをruntime authorizationに使う
- providerを自動install/updateする
- provider releaseごとのCatalog公開を要求する
- unverifiedという理由だけでmutationを追加拒否する
- provider versionから互換性を推測する
```

### MIG-802: 確認済みversion documentを参考情報へ変更する

`PROVIDER_COMPATIBILITY.md`を`TESTED_PROVIDERS.md`へ置き換える。

推奨table:

| Provider | Observed version | Components | Platform | agentmux commit | Tested at | Scope | Result |
|---|---|---|---|---|---|---|---|

冒頭に次を明記する。

```text
この表は過去にテストした組み合わせの参考情報です。
runtime allowlist、互換性保証、recommended channel、サポート期限ではありません。
表にない新versionもagentmuxは標準で起動を試みます。
メンテナはprovider releaseごとにこの表を更新する義務を負いません。
```

運用:

- 自動candidate discoveryなし
- scheduled workflowなし
- 署名なし
- expiryなし
- 更新はacpxx変更時または任意の実測時だけ
- 古い記録が残っていてもruntimeへ影響しない

### MIG-803: user-facing docsを一括更新する

更新対象:

- README
- installation
- provider setup
- CLI reference
- Rust API
- troubleshooting
- upgrade policy
- release procedure
- architecture
- status
- Definition of Done
- non-goals
- conformance

禁止表現:

- qualified provider
- verified version
- Catalog-driven
- recommended exact identity
- compatibility update
- unverified mutation
- provider promotion

推奨表現:

- observed identity
- tested combination
- local assertion
- runtime protocol negotiation
- driver contract
- best-effort support

---

## Phase 9 — validation

## 9.1 必須unit / integration test

### Version自由化

1. 未知の将来version `9999.0.0`がassertionなしで起動する
2. 非semver versionがassertionなしで起動する
3. version probeのnonzero exitでもassertionなしで起動する
4. version probe timeoutでもassertionなしで起動する
5. package metadata欠落でもassertionなしで起動する

### Local assertions

6. version一致で起動する
7. version不一致でspawn前に失敗する
8. version assertion設定時のprobe不能が明示的に失敗する
9. component一致で起動する
10. component不一致で失敗する
11. launch SHA-256一致で起動する
12. launch SHA-256不一致で失敗する

### Artifact safety

13. group/world writable executableを拒否する
14. non-regular fileを拒否する
15. executable bitなしを拒否する
16. observation後、spawn前にartifactを差し替えると失敗する
17. supplementary metadata差し替えは、それがassertion対象なら失敗する

### ACP negotiation

18. ACP v1合意でsession作成へ進む
19. unsupported protocol responseで明示的に失敗する
20. initialize responseのagentInfoを記録する
21. capabilitiesを記録する
22. required capability欠落時に該当操作を拒否する
23.未知versionでもpermission allow/denyの挙動が同一

### Continuity

24. same Agentのfollowupでfingerprintが維持される
25. disk上のprovider更新がlive Agentを暗黙に置換しない
26. 更新後の新規Agentが新identityを持つ
27. process/session変更時はcontinuity loss

### Persistence / migration

28. v1 config → final v2
29. Catalog-era verified config → floating final v2
30. Catalog-era experimental config → floating final v2
31. Catalog-era exact config → local assertion
32. v1 SQLite → v3
33. Catalog-era SQLite v2 → v3
34. old receiptのidentity情報を損なわずCatalog fieldsを除去
35. migration failureで元config/DBが保護される

### API / IPC / CLI

36. SpawnRequest JSONにCatalog fieldsがない
37. list responseにCatalog fieldsがない
38. receipt JSONに`provider_identity`がある
39. Catalog-era commandがCLI helpにない
40. `provider inspect`が未知versionを観測表示する
41. doctorがCatalogを要求しない

## 9.2 既存回帰gate

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

維持する既存suite:

- mock ACP black-box
- startup failure
- continuity
- interruption/deadline
- wait_any / wait_all
- IPC
- persistence
- process tree cleanup
- provider fault conformance
- permission audit
- capability audit
- lifecycle soak
- release benchmark
- reproducible package

## 9.3 実provider smoke

acpxx release時には、利用可能な認証環境で各driverを少なくとも一度確認する。

- Codex
- Claude
- Grok
- Cursor

これは「そのversionを中央認定する作業」ではない。acpxxのdriver変更が実providerで壊れていないことを確認するrelease smokeであり、providerの新releaseごとに再実施・公開する義務を設けない。

結果を記録する場合は`TESTED_PROVIDERS.md`へ参考情報として追記する。

## 9.4 静的完了gate

```bash
# runtime Catalog symbolsがない
rg -n 'CompatibilityCatalog|VerifiedCatalog|CatalogStore|VersionPolicy|CompatibilityLevel' src

# Catalog CLI/configがない
rg -n 'compatibility update|catalog_entry|version_policy|allow_unverified_mutations' src README.md docs

# Catalog assets/workflowがない
test ! -d compatibility
test ! -f .github/workflows/compatibility-publish.yml
test ! -f .github/workflows/provider-candidate-discovery.yml
test ! -f .github/workflows/provider-qualification.yml

# Catalog-only crypto/network dependencyがない
cargo tree -i ed25519-dalek && exit 1 || true
cargo tree -i reqwest && exit 1 || true
```

historical ADRとlegacy migration fixtureだけは明示allowlistにする。

---

## 10. 推奨commit構成

各commitは可能な限りbuildableに保つ。

1. `revert: remove signed provider compatibility catalog`
   - merge commit `9a9a982`の機械的revertのみ

2. `docs: freeze runtime-negotiated compatibility contract`
   - ADR
   - Product Contract
   - scope guard

3. `refactor: separate provider observation from authorization`
   - generic identity型
   - best-effort probes
   - artifact safety

4. `feat: add optional local provider assertions`
   - evaluator
   - failure code
   - tests

5. `refactor: gate provider sessions on ACP negotiation`
   - Catalog resolver除去
   - permission分離
   - runtime identity event

6. `refactor: replace provider locks with execution identity`
   - model
   - receipt
   - continuity fingerprint
   - actor/registry

7. `refactor: finalize Catalog-free API IPC and CLI`
   - SpawnRequest
   - list
   - inspect/doctor
   - IPC v2 final shape

8. `migrate: finalize provider config schema v2`
   - v1 migration
   - Catalog-era v2 migration
   - backup/atomic write

9. `migrate: move metadata persistence to schema v3`
   - DB migration
   - JSON migration
   - fixtures

10. `chore: delete Catalog operations assets and dependencies`
    - workflows
    - scripts
    - compatibility tree
    - Cargo dependencies
    - packaging

11. `docs: publish tested-version information as non-authoritative`
    - TESTED_PROVIDERS
    - all user docs
    - release notes

12. `test: complete floating-provider migration gates`
    - future version
    - malformed probe
    - assertions
    - ACP negotiation
    - static no-Catalog gate

PRレビューではcommit 1だけを評価せず、最終treeと全gateを評価する。

---

## 11. dependency graph

```text
MIG-000 ─┬─> MIG-001 ─> MIG-100 ─> MIG-101
         │
         └──────────────────────────────┐
                                        v
MIG-200 ─> MIG-201 ─> MIG-202 ─> MIG-203
                                        │
                                        v
MIG-300 ─> MIG-301 ─> MIG-302 ─> MIG-303
                    │                   │
                    └──────┬────────────┘
                           v
                 MIG-400 / 401 / 402 / 403
                           │
                  ┌────────┴────────┐
                  v                 v
            MIG-500..503      MIG-600..602
                  └────────┬────────┘
                           v
                    MIG-700..702
                           │
                           v
                    MIG-800..803
                           │
                           v
                     validation
```

並列化可能:

- config migrationとSQLite migration
- CLI docsとRust API docs
- Catalog asset削除準備とtest fixture作成
- provider別best-effort probe test

順序固定:

- identity model確定前にAPI/DB schemaを固定しない
- runtime移行前にCatalog moduleを完全削除しない
- migration fixtureを保存する前にrevertしない
- full validation前にmainへmergeしない

---

## 12. リスクと対策

| リスク | 対策 |
|---|---|
| 全revertでCatalog非依存のhardeningも失う | 77ファイルsalvage manifestと既存testを先に保存 |
| version probeやpackage layout変更がlatest providerを止める | probeとsupplementary metadataをbest effort化 |
| Catalog撤去がsecurity撤去と誤解される | file ownership/mode/hash/TOCTOU、permission、process isolationを必須で維持 |
| 未知versionでmutationが危険になる | versionではなく全provider共通permission policyで常に制御 |
| local pinがmigrationで失われる | v1 pinとCatalog exactをlocal assertionへ変換 |
| Catalog-era config schema 2と最終schema 2を誤認 | top-level/field shapeを先に判別するlegacy parser |
| SQLite migrationで履歴を失う | transaction、fixture、backup、round-trip test |
| `provider_lock`名称だけが残り設計意図が混乱 | code・JSON・DB columnを`provider_identity`へ統一 |
| tested-version表がallowlistと誤読される | non-authoritative disclaimer、runtime非参照、署名/expiryなし |
| ACP v2 draft対応まで同時に始めて肥大化 | 明示的non-goal。stable ACP v1のみ |
| provider plugin systemへ拡張して過剰実装 | closed four-provider setを維持 |
| semver互換性推論を追加して再び認可表になる | exact local assertion以外のversion policyを禁止 |
| online registry / telemetryを追加する | 本移行のnon-goalとして禁止 |

---

## 13. scope guard

次は、このPRでは実装しない。

- ACP v2対応
- provider plugin API
- provider registry
- auto-install
- auto-update
- update notification
- latest version discovery
- semver range
- stable/beta/nightly channel
- central blocklist
- vulnerability feed
- remote telemetry
- provider fallback
- cross-platform拡張
- arbitrary user-defined command/args/env
- generalized policy engine
- Catalog互換shimの恒久維持
- provider-specific workaroundの先回り実装

実際のprovider breakageが確認されるまでdriver workaroundを追加しない。

---

## 14. merge gate

以下がすべて満たされるまでmergeしない。

### Product behavior

- [ ] repositoryに一度も記載されていないversionを追加flagなしで起動できる
- [ ] version probeが失敗してもassertionなしならACP initializeへ進む
- [ ] local version/component/digest assertionが機能する
- [ ] ACP protocol negotiationが実行時gateになっている
- [ ] capability不足が操作単位で明示される
- [ ] permission policyがversionに依存しない
- [ ] acpxxはproviderをinstall/updateしない
- [ ] provider releaseだけを理由にacpxx変更が必要ない

### Runtime / data

- [ ] `ResolvedProviderLock`がない
- [ ] `ProviderExecutionIdentity`がAgent/Run/receiptへ記録される
- [ ] artifact差し替え検知が残っている
- [ ] config v1とCatalog-era v2を移行できる
- [ ] SQLite v1/v2をschema v3へ移行できる
- [ ] IPC/JSONにCatalog fieldsがない

### Operations

- [ ] signed Catalogがない
- [ ] signing keyがない
- [ ] sequence/expiry/rollback protectionがない
- [ ] candidate discovery workflowがない
- [ ] qualification/publish workflowがない
- [ ] Catalog update commandがない
- [ ] Catalog-only dependencyがない
- [ ] packageにCatalog assetがない

### Quality

- [ ] fmt
- [ ] clippy `-D warnings`
- [ ] all-target tests
- [ ] migration fixtures
- [ ] future-version tests
- [ ] malformed-probe tests
- [ ] assertion tests
- [ ] TOCTOU tests
- [ ] ACP negotiation tests
- [ ] process lifecycle tests
- [ ] persistence tests
- [ ] IPC tests
- [ ] release package reproducibility
- [ ] static no-Catalog gate

---

## 15. release手順

1. migration PRをsingle squashではなく、少なくとも機械的revertとforward rebuildが追跡できる履歴でmergeする。
2. `main`でfull gateを再実行する。
3. clean user environmentで、Catalog cacheやsignature fileなしにinstall・serve・spawnできることを確認する。
4. v1 config migrationとCatalog-era v2 config migrationを実fixtureで確認する。
5. SQLite migrationをcopyしたDBで確認する。
6. `v2.0.0` release notesにbreaking changeを明記する。
7. READMEの保証範囲を最終確認する。
8. `TESTED_PROVIDERS.md`は実測できた範囲だけ記載し、空欄を無理に埋めない。
9. `v2.0.0`をtag・releaseする。
10. release後、providerの新versionが出ても、driver contractが壊れた実例がない限り追従issueや更新PRを作らない。

### release notesに必ず含める内容

- Compatibility Catalogを撤去した
- 未知versionを標準で拒否しない
- version/digest固定はlocal assertionsへ移った
- `compatibility update/status`を削除した
- `provider inspect`を追加した
- config migration方法
- SQLite migration
- Catalog-era transient buildとのAPI/IPC互換性は保証しない
- providerの破壊的変更時だけdriver updateが必要になる可能性

---

## 16. 完了後の正式な契約文

READMEとProduct Contractには、次の趣旨を採用する。

> agentmuxは、中央で認定されていないprovider versionであることだけを理由に実行を拒否しない。組み込みProviderDriverの起動契約、ローカルartifact安全性、任意のユーザーassertion、ACP protocol negotiation、および必要capabilityが成立する限り、新しいprovider versionを標準経路で起動する。agentmuxはproviderをinstallまたはupdateせず、メンテナはprovider releaseごとのversion認定・公開を行わない。providerが起動方法、認証、ACP protocol、permission semantics、またはdriver interfaceを破壊的に変更した場合は、ProviderDriverの更新が必要になることがある。

確認済みversion情報には、次を添える。

> Tested provider information is observational and non-authoritative. It is not a runtime allowlist, compatibility guarantee, recommended channel, support lifecycle, or maintainer commitment to track provider releases.

---

## 17. 最終判断

この移行は、PR #4を部分修正して`Experimental`を標準化する方式では完了しない。Catalogのschema、署名、更新、qualification、public type、failure taxonomy、運用workflowが残り、使わない中央認定機構を維持することになるためである。

したがって完了形は次の一文で定義する。

> **PR #4を機械的にrevertし、旧exact pinも撤去した上で、provider versionを実行認可から観測情報と任意のlocal assertionへ移し、ACP initialize/capability negotiationを実行時互換性のgateにする。**
