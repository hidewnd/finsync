# MySQL Batch Administration Tool

**English** | [简体中文](README.zh-CN.md)

A lightweight desktop application for batch MySQL database administration, built with **Tauri + Rust**. Runs as a single executable with no additional environment setup.

## ✨ Features

| Feature | Description |
|---------|-------------|
| 🔗 Multiple connections | Configure multiple MySQL instances in `config.ini` and switch between them using the dropdown |
| 🔍 Pattern matching | Match target databases by combining prefix and suffix filters with AND logic |
| ✅ Batch selection | Matching databases are selected by default, with select-all and deselect-all controls |
| 📝 SQL editor | Multiline input with the Ctrl+Enter shortcut to execute SQL |
| 🔒 Transactions | Run `START TRANSACTION → EXEC → COMMIT/ROLLBACK` per database; a failure in one database does not affect the others |
| 📋 Live logs | Green for success and red for failure, with timestamps and elapsed time |
| 🌐 JDBC URLs | Support both `jdbc:mysql://host:port/?params` and traditional host/port configuration |
| 🎨 Dark theme | Native desktop window without browser chrome |

![Application screenshot](https://lyne-bucket.oss-cn-shanghai.aliyuncs.com/notes/202604291441562.png)

## 📦 Installation

### Option 1: Installer (recommended)

Download the `MySQL 批量运维工具_*.exe` installer and double-click it to install.

### Option 2: Portable executable

Download `finsync.exe`, place it in the same directory as `config.ini`, and double-click it to run.

> Requires Windows 10 or later (with the WebView2 runtime included).

## 🔧 Configuration

Create `config.ini` in the same directory as the executable:

```ini
; Format 1 — Traditional fields
[Local Development]
host=127.0.0.1
port=3306
user=root
password=root

; Format 2 — JDBC URL (url takes precedence over host/port)
[Alibaba Cloud RDS]
url=jdbc:mysql://rm-bp1xxxxx.mysql.rds.aliyuncs.com:3306/?characterEncoding=utf8&useSSL=false
user=admin
password=your_password

; Both formats can be used in the same file
[Test Environment]
host=192.168.1.100
port=3306
user=test_user
password=test_pass
```

> After adding a connection, click “🔄 刷新配置” (Refresh Configuration) in the app to load it without restarting.

## 🚀 Development

### Prerequisites

- [Rust](https://www.rust-lang.org/) 1.70+
- [Node.js](https://nodejs.org/) 18+
- Windows 10 or later

### Quick start

```bash
# Clone the repository
git clone https://github.com/your-username/finsync.git
cd finsync

# Install frontend dependencies
npm install

# Build the frontend
npm run build

# Start development mode with hot reload
cargo tauri dev

# Build for production
cargo tauri build -b nsis
```

### Project structure

```text
finsync/
├── src-tauri/           # Rust backend
│   └── src/
│       ├── main.rs      # Entry point (windows_subsystem hides the console)
│       ├── lib.rs       # Tauri Builder and command registration
│       ├── config.rs    # INI and JDBC URL parsing
│       ├── db.rs        # MySQL connections, transactions, and character set settings
│       └── commands.rs  # Tauri IPC commands and live event streams
├── index.html          # Dark-themed UI
├── app.js              # Frontend logic (Tauri invoke and events)
├── style.css           # Styles
└── config.ini          # Example configuration
```

## 🛠 Technology stack

| Layer | Technology |
|-------|------------|
| Desktop framework | Tauri 2.x |
| Backend | Rust + mysql crate |
| Frontend | Vite + Vanilla JS |
| Packaging | NSIS |
| Character set | utf8mb4 (configured for both the connection pool and transactions) |

## 📄 License

[MIT](LICENSE)
