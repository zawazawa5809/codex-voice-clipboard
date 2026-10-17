# Codex Voice Clipboard

Codex Desktop が保存した音声文字起こしを、新しく増えたらクリップボードへ写す常駐アプリです。貼り付けは自分で行います（Ctrl+V / ⌘V）。Codex 本体は変えず、別アプリへの自動貼り付けもしません。

Windows と macOS（14 以降・Apple Silicon）で同じ動きをします。Tauri 2（裏の処理は Rust、設定などの小さな画面は TypeScript）で作っています。仕様の正本は [docs/spec/cross-platform.md](docs/spec/cross-platform.md) です。

## 使い方（両OS共通）

起動すると `~/.codex/transcription-history.jsonl` を読み取り専用で見張り、新しい文字起こしを検出してから 2 秒後にコピーします。起動前からあった履歴は自動コピーせず、メニューから手でコピーできます。

メニューはトレイ（Windows）／メニューバー（macOS）のアイコンから開きます。

| 項目 | 動き |
|---|---|
| 状態表示 | 監視中／一時停止／エラー。アイコンの色と形でも示す（通常・琥珀・赤、明暗2種） |
| 自動コピー | OFF にすると待機中のコピーも取り消す。監視と最近10件の更新は続け、再開しても停止中の履歴はコピーしない |
| 最新をコピー／最近10件からコピー | 待たずにすぐコピーする |
| コピーまでの待機時間 | 0〜60 秒、0.1 秒刻み。変えると待機中のコピーを取り消す |
| ログイン時に自動起動 | OS 側で無効にされていたら勝手に戻さず、OS の設定を開くか尋ねる |
| ログを表示 | 時刻とイベント名だけのログ。本文は書かない |
| クリップボード上書きについて | 初回起動時にも表示する注意 |
| アンインストール… | 自動起動の登録・設定・ログを消す（下の「削除」） |
| 終了 | 監視と常駐を止める。自動起動の設定は変えない |

**待機中に別の内容をコピーすると、文字起こしで上書きされます。** 気になるときは自動コピーを OFF にして手動でコピーしてください。一度コピーした履歴を繰り返しコピーすることはありません。

## プライバシー

- 本文はクリップボードと、メモリ上の最近10件にだけ置きます。ログ・設定には書きません。
- ネットワーク通信をしません。自動更新の部品も入れず、画面の読み込みも外部への接続を禁止しています（CSP）。
- OS のクリップボード同期（macOS のユニバーサルクリップボード、Windows のクリップボード履歴・クラウド同期）が有効だと、本文が他の機器や履歴に渡ることがあります。このアプリはそれらの設定を変えません。

## macOS

- `.dmg` から `Codex Voice Clipboard.app` を `~/Applications`（または `/Applications`）へ置きます。Dock には出ず、メニューバーにだけ出ます。
- **署名していません。** 初回は開けないので、システム設定 →「プライバシーとセキュリティ」→「このまま開く」を押してください（macOS 15 以降は右クリック→「開く」では開けません）。
- 設定は `~/Library/Application Support/CodexVoiceClipboard`、ログは `~/Library/Logs/CodexVoiceClipboard` です。
- 自動起動は `~/Library/LaunchAgents/CodexVoiceClipboard.plist` で登録します。システム設定 →「一般」→「ログイン項目」で無効にできます。
- 削除: メニューの「アンインストール…」が、自動起動・設定・ログを消したあと `.app` 本体も消して終了します（`/Applications` か `~/Applications` に置いたときだけ）。

## Windows

- NSIS のインストーラでユーザー単位に入れます（管理者権限は不要）。署名していないため、発行元不明の警告が出ることがあります。
- 設定とログは `%LOCALAPPDATA%\CodexVoiceClipboard` です。
- 自動起動は `HKCU\...\Run\CodexVoiceClipboard` に登録します。「設定 → アプリ → スタートアップ」での無効化（`StartupApproved\Run`）を読み取り、勝手に戻しません。
- 削除: 「設定 → アプリ」またはメニューの「アンインストール…」から、インストーラ付属のアンインストーラを使います。アンインストーラは先に `--uninstall` でアプリ自身の後片付けを呼びます。

## 開発

必要なもの: Rust（stable）、Node.js と pnpm、Python 3 と uv（テスト・アイコン生成）。

```bash
pnpm install
cargo test --workspace
pnpm tauri build --bundles app --target aarch64-apple-darwin
uv run --no-project python tests/integration.py "target/aarch64-apple-darwin/release/bundle/macos/Codex Voice Clipboard.app"
uv run --with pillow python -m unittest tests/test_icons.py tests/test_no_network.py
```

- `core/` — OS に依存しない中身（履歴の読み取り、自動コピーの予約、ログ、設定）。テストの大半はここ。
- `src-tauri/` — トレイ、クリップボード、自動起動、多重起動防止。OS に触る部分はすべて差し替え可能な窓口（Rust のトレイト）を通し、本物と `--sandbox` 用を起動時に選ぶ。
- `src/` — 待機時間・ログ・注意の小さな画面（TypeScript）。
- アイコンは `scripts/generate_icons.py` で作り、`pnpm tauri icon src-tauri/icons/app-source.png` で配布用に変換します。

### 隔離テスト用の引数

| 引数 | 意味 |
|---|---|
| `--sandbox <絶対パス>` | 履歴・設定・クリップボード・自動起動・アプリ本体を、そのフォルダ内のファイルに置き換える。本物のクリップボードやログイン項目には触れない |
| `--no-watcher` | `--sandbox` 専用。変更通知を使わず、2 秒ごとの定期確認だけで動かす |
| `--uninstall` | アプリが作ったもの（自動起動の登録・設定・ログ）を消す。確認を出す。`--sandbox` と一緒なら確認を省き、そのフォルダの中だけを消す |

`tests/integration.py` は `--sandbox` で起動したアプリだけを相手にし、合成した履歴で16項目を確かめます。テスト用のコピー先 `clipboard.txt` には合成した本文が残ります。

### まだ確かめていないこと

- Windows 版はこの Mac ではビルドできません（Windows のリソースコンパイラが無い）。型検査と lint は空のリソースを返す代用品で通していますが、実行ファイルとインストーラは Windows 機か CI で作り、隔離テストを当てる必要があります。
- 本物のクリップボード・ログイン項目・OS 側の無効化検出・`.app` の自己削除・NSIS の削除経路は、実機での確認が必要です。隔離テストの通過をこれらの確認済みとは扱わないでください。
