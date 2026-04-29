# MySQL 批量运维工具

基于 **Tauri + Rust** 构建的轻量级 MySQL 数据库批量运维桌面应用。单 exe 运行，零环境依赖。

## ✨ 功能

| 功能 | 说明 |
|------|------|
| 🔗 多连接管理 | 通过 `config.ini` 配置多个 MySQL 实例，界面下拉切换 |
| 🔍 模糊匹配 | 前缀 + 后缀 AND 逻辑匹配目标数据库 |
| ✅ 批量勾选 | 匹配结果带勾选框，默认全选，支持全选/取消 |
| 📝 SQL 编辑器 | 多行输入，Ctrl+Enter 快捷执行 |
| 🔒 事务包裹 | 逐库 `START TRANSACTION → EXEC → COMMIT/ROLLBACK`，单库失败不影响其他库 |
| 📋 实时日志 | 成功绿 / 失败红，含时间戳和耗时 |
| 🌐 JDBC URL | 支持 `jdbc:mysql://host:port/?params` 和传统 host/port 两种配置格式 |
| 🎨 深色主题 | 原生桌面窗口，无浏览器边框 |



![image-20260429144103490](https://lyne-bucket.oss-cn-shanghai.aliyuncs.com/notes/202604291441562.png)

## 📦 安装

### 方式一：安装包（推荐）

下载 `MySQL 批量运维工具_*.exe` 安装包，双击安装即可。

### 方式二：免安装

下载 `finsync.exe`，连同 `config.ini` 放到同一目录，双击运行。

> 需要 Windows 10+ 系统（WebView2 运行时内置）。

## 🔧 配置

在 exe 同目录创建 `config.ini`：

```ini
; 格式1 — 传统字段
[本地开发]
host=127.0.0.1
port=3306
user=root
password=root

; 格式2 — JDBC URL（url 优先于 host/port）
[阿里云RDS]
url=jdbc:mysql://rm-bp1xxxxx.mysql.rds.aliyuncs.com:3306/?characterEncoding=utf8&useSSL=false
user=admin
password=your_password

; 多种格式可以混合使用
[测试环境]
host=192.168.1.100
port=3306
user=test_user
password=test_pass
```

> 添加新连接后点击界面「🔄 刷新配置」即可加载，无需重启。

## 🚀 开发

### 环境要求

- [Rust](https://www.rust-lang.org/) 1.70+
- [Node.js](https://nodejs.org/) 18+
- Windows 10+ 系统

### 快速开始

```bash
# 克隆项目
git clone https://github.com/your-username/finsync.git
cd finsync

# 安装前端依赖
npm install

# 构建前端
npm run build

# 开发模式（热重载）
cargo tauri dev

# 生产构建
cargo tauri build -b nsis
```

### 项目结构

```
finsync/
├── src-tauri/           # Rust 后端
│   └── src/
│       ├── main.rs      # 入口（windows_subsystem 隐藏终端）
│       ├── lib.rs        # Tauri Builder + 命令注册
│       ├── config.rs     # INI + JDBC URL 双格式解析
│       ├── db.rs         # MySQL 连接 + 事务执行 + 字符集设置
│       └── commands.rs   # Tauri IPC 命令 + 实时事件流
├── index.html           # 深色主题 UI
├── app.js               # 前端逻辑（Tauri invoke + events）
├── style.css            # 样式
└── config.ini            # 示例配置
```

## 🛠 技术栈

| 层 | 技术 |
|---|---|
| 桌面框架 | Tauri 2.x |
| 后端 | Rust + mysql crate |
| 前端 | Vite + Vanilla JS |
| 打包 | NSIS |
| 字符集 | utf8mb4（连接池 + 事务双重设置） |

## 📄 许可证

[MIT](LICENSE)
