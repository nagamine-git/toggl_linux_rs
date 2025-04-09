# toggl_linux_rs

Linuxデスクトップ環境で自動的に活動をトラッキングし、Togglへ登録するRustアプリケーション。

## 概要

このプログラムは以下の機能を提供します：

1. 1分ごとにLinuxデスクトップ（Xfce）上でのアクティブウィンドウのタイトル情報を収集
2. Googleカレンダーから予定情報を取得
3. 収集した情報を15分ごとに分析し、ユーザーの活動を推定
4. 推定精度が50%以上の場合、自動的にToggl APIを使って時間記録を登録
5. 推定精度が50%未満の場合、確度の高い候補を提示するか新規イベント作成を促す
6. オフライン時は簡易な推論ロジックを使用

## システム設計

### 全体アーキテクチャ

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│  データ収集     │     │  分析エンジン   │     │  イベント登録   │
│ ・ウィンドウ情報│ ──> │ ・GPT-4o mini  │ ──> │ ・Toggl API     │
│ ・カレンダー情報│     │ ・ローカル推論  │     │ ・ユーザー確認  │
└─────────────────┘     └─────────────────┘     └─────────────────┘
```

### コンポーネント

1. **データコレクター**
   - 1分ごとにアクティブウィンドウタイトルを`xdotool`または`xprop`を使用して取得
   - Googleカレンダーから予定情報を定期的に同期
   - 収集されたデータをローカルストレージに保存

2. **分析エンジン**
   - 15分ごとに蓄積されたデータを分析
   - オンライン時はGPT-4o miniを使用して活動内容と確度を推定
   - オフライン時はキーワードマッチングと簡易な学習モデルを使用

3. **イベント管理**
   - Toggl APIとの連携
   - 確度の高い推定結果を自動的に登録
   - 確度の低い推定結果をユーザーに提示

4. **設定・UI**
   - 設定ファイルによるカスタマイズ
   - 簡易的なUI（通知やコマンドライン）

## 技術スタック

- **言語**: Rust
- **外部ツール**: xdotool/xprop（ウィンドウ情報取得）
- **API統合**:
  - OpenAI API (GPT-4o mini)
  - Toggl API
  - Google Calendar API
- **データ保存**: SQLite/ローカルファイル

## 実装計画

### フェーズ1：基本機能実装
1. プロジェクト初期化とライブラリ選定
2. ウィンドウタイトル取得機能の実装
3. Toggl APIクライアントの実装
4. Google Calendar APIクライアントの実装
5. 簡易的なデータ保存機構の実装

### フェーズ2：推論エンジン実装
1. OpenAI API連携の実装
2. 簡易ローカル推論エンジンの実装
3. 活動推定ロジックの実装

### フェーズ3：統合と最適化
1. コンポーネント統合
2. エラーハンドリング強化
3. 設定機能の実装
4. UI/通知機能の実装

### フェーズ4：拡張機能
1. プロセス情報取得機能の追加
2. 機械学習モデルの精度向上
3. バッチ処理機能の実装

## 依存ライブラリ（予定）

```toml
[dependencies]
# HTTP & API
reqwest = { version = "0.11", features = ["json"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# Google Calendar
google-calendar3 = "5.0"
yup-oauth2 = "8.3"

# OpenAI
async-openai = "0.12"

# データベース
rusqlite = "0.28"
r2d2 = "0.8"
r2d2_sqlite = "0.21"

# Linux/X11
x11rb = "0.11"
# 又は
xdotool = "0.0.1"  # Rustラッパー（存在すれば）

# ユーティリティ
chrono = "0.4"
log = "0.4"
env_logger = "0.10"
config = "0.13"
anyhow = "1.0"
thiserror = "1.0"
```

## セットアップ手順（予定）

1. 必要なシステム依存関係をインストール
   ```bash
   sudo apt install xdotool libx11-dev libsqlite3-dev
   ```

2. APIキーを取得
   - OpenAI API
   - Toggl API
   - Google Calendar API

3. 設定ファイルを作成
   ```bash
   cp config.example.toml config.toml
   # 設定ファイルを編集し、APIキーなどを設定
   ```

4. アプリケーションをビルド
   ```bash
   cargo build --release
   ```

5. 自動起動の設定
   ```bash
   # Xfceの自動起動に追加する手順
   ```

## ライセンス

MIT

## 貢献

プルリクエスト、イシュー報告歓迎します！

## 使い方

### インストール

1. Rustがインストールされていない場合は、[rustup](https://rustup.rs/)からインストールしてください：
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. 必要なシステム依存関係をインストールします：
   ```bash
   sudo apt install xdotool libx11-dev libsqlite3-dev
   ```

3. このリポジトリをクローンします：
   ```bash
   git clone https://github.com/nagamine-git/toggl_linux_rs.git
   cd toggl_linux_rs
   ```

4. リリースビルドをコンパイルします：
   ```bash
   cargo build --release
   ```

### 設定

1. サンプル設定ファイルをコピーして編集します：
   ```bash
   cp config.example.toml config.toml
   ```

2. `config.toml` を編集して必要な情報を設定します：

   ```toml
   [general]
   # データ保存ディレクトリ
   log_dir = "~/.local/share/toggl_linux_rs/logs"
   # ポーリング間隔（秒）
   polling_interval_seconds = 60
   # アイドル検出の閾値（秒）
   idle_threshold_seconds = 300

   [toggl]
   # Toggl APIトークン（Togglのプロフィール設定ページから取得できます）
   api_token = "your_toggl_api_token"
   # ワークスペースID（Toggl APIまたはウェブインターフェースから確認できます）
   workspace_id = 1234567
   # 無効なプロジェクトとしてマークするプロジェクト名
   invalid_projects = ["休憩", "ミーティング準備"]

   # OpenAI API設定（任意）
   # 設定しない場合は簡易的なローカル推論エンジンが使用されます
   [openai]
   api_key = "your_openai_api_key"
   model = "gpt-4o-mini"

   # Google Calendar API設定（任意）
   # 設定しない場合はカレンダー情報は使用されません
   [google_calendar]
   credentials_path = "~/path/to/your/google_credentials.json"
   calendar_id = "your_calendar_id@group.calendar.google.com"
   ```

   各設定項目の説明：
   - `general.log_dir`: 活動ログを保存するディレクトリ
   - `general.polling_interval_seconds`: データを収集する間隔（秒）
   - `general.idle_threshold_seconds`: アイドル状態とみなす閾値（秒）
   - `toggl.api_token`: TogglのAPIトークン
   - `toggl.workspace_id`: 使用するTogglのワークスペースID
   - `toggl.invalid_projects`: 自動登録から除外するプロジェクト名
   - `openai.api_key`: OpenAI APIキー（オプション）
   - `openai.model`: 使用するOpenAIのモデル名
   - `google_calendar.credentials_path`: Google APIのクレデンシャルファイルパス
   - `google_calendar.calendar_id`: 使用するGoogleカレンダーのID

### 起動方法

基本的な起動方法：
```bash
cargo run
```

リリースビルドで実行（推奨）：
```bash
cargo run --release
```

特定の設定ファイルを指定して起動：
```bash
cargo run --release -- -c /path/to/your/config.toml
```

バイナリを直接実行：
```bash
./target/release/toggl_linux_rs
```

ヘルプを表示：
```bash
cargo run -- --help
```

### 自動起動の設定

Xfceデスクトップ環境での自動起動の設定方法：

```bash
mkdir -p ~/.config/autostart
cat > ~/.config/autostart/toggl_linux_rs.desktop << EOF
[Desktop Entry]
Type=Application
Name=Toggl Linux Tracker
Comment=自動活動トラッキングアプリケーション
Exec=/path/to/toggl_linux_rs
Terminal=false
Hidden=false
X-GNOME-Autostart-enabled=true
EOF
```

### トラブルシューティング

1. **アプリケーションが起動しない場合**
   - 設定ファイルが正しい場所にあるか確認してください
   - APIトークンやキーが正しく設定されているか確認してください
   - ログファイルを確認してエラーメッセージを確認してください

2. **ウィンドウ情報が取得できない場合**
   - xdotoolがインストールされているか確認してください
   - X11環境で実行されているか確認してください

3. **Togglへの登録が機能しない場合**
   - TogglのAPIトークンとワークスペースIDが正しいか確認してください
   - インターネット接続を確認してください

4. **OpenAI APIエラー**
   - APIキーが正しく設定されているか確認してください
   - APIの利用制限に達していないか確認してください

5. **よくあるエラーメッセージ**
   - `Error: Configuration file not found`: 設定ファイルが見つかりません。`config.toml`が正しい場所にあるか確認してください
   - `Error: Failed to authenticate with Toggl API`: TogglのAPIトークンが無効です
   - `Error: OpenAI API request failed`: OpenAI APIへのリクエストが失敗しました