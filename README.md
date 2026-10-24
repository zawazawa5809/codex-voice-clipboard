# Codex Voice Clipboard

Codex Desktop の音声入力で話した内容を、自動でクリップボードへ写す常駐アプリです。Codex の入力欄から文字起こしが消えてしまっても、別の場所に Ctrl+V / ⌘V で貼り付けて取り戻せます。Windows と macOS で同じように動きます。

> **English summary** — A tray / menu-bar app that copies each new Codex Desktop voice transcript to the clipboard, so you can paste it anywhere if it gets lost. Download the Windows installer or the macOS `.dmg` from [Releases](https://github.com/zawazawa5809/codex-voice-clipboard/releases/latest). Neither is code-signed: on Windows choose "More info → Run anyway"; on macOS open it once, then click "Open Anyway" in System Settings → Privacy & Security. It reads `~/.codex/transcription-history.jsonl` read-only, never writes transcripts to logs or settings, and makes no network connections. Unofficial; not affiliated with OpenAI.

**非公式のツールです。** OpenAI とは関係がありません。Codex が手元に保存する履歴ファイル（`~/.codex/transcription-history.jsonl`）を読むだけで、Codex 本体には手を加えません。このファイルの形式は公開された仕様ではないため、Codex の更新で読めなくなることがあります。

## 動作環境

| OS | 対応 |
|---|---|
| Windows | Windows 10 / 11（x64） |
| macOS | macOS 14 以降、Apple Silicon（M1 以降）。Intel Mac では動きません |

Codex Desktop で音声入力を一度でも使い、上の履歴ファイルができていることが前提です。

## インストール

[Releases](https://github.com/zawazawa5809/codex-voice-clipboard/releases/latest) から自分の OS のファイルをダウンロードします。どちらも**コード署名をしていない**ため、初回だけ OS の警告を越える操作が要ります。ダウンロードが途中で壊れていないかは、同じページの `SHA256SUMS.txt` と照らし合わせて確かめられます。

### Windows

1. `CodexVoiceClipboard_<版>_windows_x64-setup.exe` を実行します。
2. 「Windows によって PC が保護されました」と出たら、「詳細情報」→「実行」を押します。
3. インストーラの指示に従います。自分のユーザーだけに入り、管理者権限は要りません。
4. スタートメニューから「Codex Voice Clipboard」を起動すると、タスクトレイにアイコンが出ます。

### macOS

1. `CodexVoiceClipboard_<版>_macos_arm64.dmg` を開き、`Codex Voice Clipboard.app` を「アプリケーション」フォルダへドラッグします。
2. アプリを開きます。「"Codex Voice Clipboard" は開いていません」という画面が出たら、「完了」を押して閉じます（「ゴミ箱に入れる」は押さないでください）。この時点ではまだ起動しません。設定画面も自動では開かないので、次の手順で自分で開きます。
3. 画面左上の Apple メニュー →「システム設定…」→「プライバシーとセキュリティ」を開き、右側を一番下までスクロールします。「"Codex Voice Clipboard" は開いていません」の横にある「このまま開く」を押し、パスワード（または Touch ID）で許可します。このボタンは、手順 2 からおよそ 1 時間で消えます。見当たらないときは、手順 2 からやり直してください。
4. メニューバーにアイコンが出ます（Dock には出ません）。

macOS 15 以降は、右クリック →「開く」では警告を越えられません。上の 3 の手順を使ってください。

## 使い方

起動したら、あとは Codex で普段どおり音声入力するだけです。新しい文字起こしを見つけてから 2 秒後に、クリップボードへ写します。アプリを起動する前からあった履歴は自動ではコピーせず、メニューから選んでコピーできます。

アイコンの色で状態が分かります。

| アイコン | 状態 |
|---|---|
| 通常 | 監視中 |
| 琥珀（二本線） | 自動コピーを OFF にしている |
| 赤（感嘆符） | 履歴ファイルが無い・読めない、またはコピーに失敗した |

メニューはトレイ（Windows）／メニューバー（macOS）のアイコンから開きます。

| 項目 | 動き |
|---|---|
| 自動コピー | OFF にすると待機中のコピーも取り消す。監視と最近10件の更新は続け、ON に戻しても停止中の履歴はコピーしない |
| 最新をコピー／最近10件からコピー | 待たずにすぐコピーする |
| コピーまでの待機時間 | 0〜60 秒、0.1 秒刻み。変えると待機中のコピーを取り消す |
| ログイン時に自動起動 | 初期は OFF。OS 側で無効にされていたら勝手に戻さず、OS の設定を開くか尋ねる |
| ログを表示 | 時刻とイベント名だけのログ。本文は書かない |
| クリップボード上書きについて | 初回起動時にも出す注意 |
| アンインストール… | 下の「削除」 |
| 終了 | 監視と常駐を止める。自動起動の設定は変えない |

## 気をつけること

- **待機中（既定 2 秒）に別の内容をコピーすると、文字起こしで上書きされます。** 困るときは自動コピーを OFF にして、必要なときだけメニューからコピーしてください。一度コピーした履歴を繰り返しコピーすることはありません。
- 文字起こしの本文は、クリップボードとメモリ上の最近10件にだけ置きます。ログや設定ファイルには書きません。
- このアプリはネットワーク通信をしません。自動更新もありません。新しい版は Releases から入れ直してください。
- OS のクリップボード同期（macOS のユニバーサルクリップボード、Windows のクリップボード履歴・クラウド同期）が有効だと、本文が他の機器や履歴に渡ることがあります。このアプリはそれらの設定を変えません。

## 困ったとき

| 症状 | 確かめること |
|---|---|
| アイコンがずっと赤い | Codex Desktop で音声入力を一度使ったか。履歴ファイルが作られると自動で通常に戻る |
| コピーされない | 自動コピーが OFF（琥珀）になっていないか。「ログを表示」で `clipboard-failed` が出ていないか |
| macOS で「壊れているため開けません」と出る | ダウンロードが途中で壊れた可能性がある。`SHA256SUMS.txt` と照らし合わせ、取り直す |
| Codex の更新後に動かなくなった | 履歴ファイルの形式が変わった可能性がある。Issue で知らせてください |

## 削除

どちらの OS も、メニューの「アンインストール…」から消せます。自動起動の登録・設定・ログを消したあと、アプリ本体も消して終了します。Codex の履歴やクリップボードの中身は消しません。

- **Windows**: 「設定 → アプリ → インストールされているアプリ」からも削除できます。
- **macOS**: アプリを「アプリケーション」か `~/Applications` に置いたときだけ、本体も自分で消します。それ以外の場所に置いたときは、終了してから `.app` をゴミ箱へ入れてください。

アプリが作るものは次のとおりです。

| | Windows | macOS |
|---|---|---|
| 設定・ログ | `%LOCALAPPDATA%\CodexVoiceClipboard` | `~/Library/Application Support/CodexVoiceClipboard`、`~/Library/Logs/CodexVoiceClipboard` |
| 自動起動（ON のときだけ） | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\CodexVoiceClipboard` | `~/Library/LaunchAgents/CodexVoiceClipboard.plist` |
| 画面表示部品のデータ | `%LOCALAPPDATA%\dev.zawa.codexvoiceclipboard` | — |

自動起動は、Windows の「設定 → アプリ → スタートアップ」、macOS のシステム設定 →「一般」→「ログイン項目」でも無効にできます。

## 開発

Tauri 2 で作っています（裏の処理は Rust、設定などの小さな画面は TypeScript）。

必要なもの: Rust（stable）、Node.js と pnpm、Python 3 と uv（テスト・アイコン生成）。

```bash
pnpm install
cargo test --workspace
pnpm tauri build --bundles app,dmg --target aarch64-apple-darwin
uv run --no-project python tests/integration.py "target/aarch64-apple-darwin/release/bundle/macos/Codex Voice Clipboard.app"
uv run --with pillow python -m unittest tests/test_icons.py tests/test_no_network.py tests/test_installer_hooks.py tests/test_release.py
```

Windows 版は Windows 上でしか作れません（`pnpm tauri build --bundles nsis`）。手元に Windows が無ければ GitHub Actions の `windows` ワークフローを使います。

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

### リリースの出し方

1. `src-tauri/tauri.conf.json`・`package.json`・`src-tauri/Cargo.toml`・`core/Cargo.toml` の `version` を同じ値に上げる（`tests/test_release.py` が揃っているかを確かめる）。
2. `v<版>` のタグを付けて push する。
3. `release` ワークフローが両 OS でビルドとテストを通し、インストーラ・`.dmg`・`SHA256SUMS.txt` を付けた**下書き**の Release を作る。中身を確かめてから、GitHub 上で公開する。

### まだ確かめていないこと

- ログイン時の実際の自動起動（両 OS）。
- macOS の `.app` の自己削除。
- Releases から落とした `.dmg` を、別の Mac で初めて開く流れ。

## ライセンス

[MIT](LICENSE)
