//! SQLite 数据库模块
//!
//! 采用极简双表结构，满足基座和所有插件的数据与配置存储需求：
//! - `configs`：系统配置与插件配置（联合唯一约束防重复）
//! - `datas`：剪贴板历史、OCR 结果、插件业务数据等
//!
//! 系统级配置统一使用 `pluginID = "SYS_settings"` 前缀；
//! 插件配置使用插件自身 ID（不可以 SYS 开头）。

use rusqlite::{Connection, OptionalExtension, params};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// 系统设置使用的固定 pluginID
pub const SYS_PLUGIN_ID: &str = "SYS_settings";

/// 系统设置 key：开机启动
pub const K_STARTUP: &str = "startup";
/// 系统设置 key：主题（"1" 深色，"0" 浅色）
pub const K_THEME: &str = "theme";

/// 全局数据库连接（懒加载单例）
///
/// 使用 OnceLock + Mutex 模式：
/// - OnceLock 保证初始化只执行一次
/// - Mutex 保证多线程下连接的串行访问（SQLite 单连接非线程安全）
static DB: OnceLock<Mutex<Connection>> = OnceLock::new();

/// 获取数据库存放目录
///
/// Windows 下路径形如：`%LOCALAPPDATA%\settings_app\`
/// 若获取失败则退回当前工作目录
fn db_dir() -> PathBuf {
    if let Some(proj) = directories::ProjectDirs::from("", "", "settings_app") {
        return proj.data_local_dir().to_path_buf();
    }
    PathBuf::from(".")
}

/// 获取数据库完整文件路径
fn db_path() -> PathBuf {
    let mut p = db_dir();
    p.push("settings_app.db");
    p
}

/// 获取当前计算机名（失败时退回 "unknown"）
pub fn pc_name() -> String {
    hostname::get()
        .ok()
        .and_then(|s| s.into_string().ok())
        .unwrap_or_else(|| "unknown".to_string())
}

/// 当前时间戳字符串（ISO8601 格式）
fn now_str() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// 初始化数据库：创建文件 → 建表 → 插入默认系统配置
///
/// 该函数应在程序启动时调用一次。失败时返回 rusqlite::Error。
pub fn init() -> rusqlite::Result<()> {
    // 1. 确保目录存在
    let dir = db_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::error!("创建数据库目录失败 {:?}: {:?}", dir, e);
    }

    // 2. 打开/创建数据库连接
    let path = db_path();
    tracing::info!("数据库路径: {:?}", path);
    let conn = Connection::open(&path)?;

    // 3. 建表 + 建索引
    create_schema(&conn)?;

    // 4. 插入默认配置（已存在则忽略）
    insert_default_configs(&conn)?;

    // 5. 注册到全局
    let _ = DB.set(Mutex::new(conn));
    Ok(())
}

/// 创建所有表与索引（IF NOT EXISTS 幂等）
fn create_schema(conn: &Connection) -> rusqlite::Result<()> {
    // configs 表：系统配置与插件配置
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS configs (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            userID      TEXT,
            pcName      TEXT,
            createtime  DATETIME NOT NULL,
            updatetime  DATETIME NOT NULL,
            pluginID    TEXT NOT NULL,
            k           TEXT NOT NULL,
            v_default   TEXT,
            v_actual    TEXT,
            str1        TEXT,
            str2        TEXT,
            str3        TEXT,
            int1        INTEGER,
            int2        INTEGER,
            int3        INTEGER,
            real1       REAL,
            real2       REAL,
            real3       REAL,
            synced      INTEGER DEFAULT 0,
            UNIQUE(userID, pcName, pluginID, k)
        );

        CREATE TABLE IF NOT EXISTS datas (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            userID        TEXT,
            pcName        TEXT,
            createtime    DATETIME NOT NULL,
            updatetime    DATETIME NOT NULL,
            pluginID      TEXT NOT NULL,
            dataType      TEXT,
            content_type  TEXT,
            content_str   TEXT,
            content_blob  BLOB,
            lastUse       DATETIME,
            md5_key       TEXT,
            str1          TEXT,
            str2          TEXT,
            str3          TEXT,
            int1          INTEGER,
            int2          INTEGER,
            int3          INTEGER,
            real1         REAL,
            real2         REAL,
            real3         REAL,
            blob1         BLOB,
            synced        INTEGER DEFAULT 0
        );

        CREATE INDEX IF NOT EXISTS idx_datas_plugin_type ON datas(pluginID, dataType);
        CREATE INDEX IF NOT EXISTS idx_datas_md5 ON datas(md5_key);
        CREATE INDEX IF NOT EXISTS idx_datas_lastuse ON datas(lastUse DESC);
        CREATE INDEX IF NOT EXISTS idx_configs_plugin_k ON configs(pluginID, k);
        CREATE INDEX IF NOT EXISTS idx_sync_pending ON datas(synced) WHERE synced = 0;
        "#,
    )?;
    Ok(())
}

/// 插入系统默认配置（已存在则跳过）
///
/// 默认项：
/// - startup = "1"（开机启动）
/// - theme   = "1"（深色主题）
fn insert_default_configs(conn: &Connection) -> rusqlite::Result<()> {
    let pc = pc_name();
    let now = now_str();

    // 使用 INSERT OR IGNORE 借助 UNIQUE(userID, pcName, pluginID, k) 约束实现幂等
    let mut stmt = conn.prepare(
        r#"INSERT OR IGNORE INTO configs
           (userID, pcName, createtime, updatetime, pluginID, k, v_default, v_actual, synced)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0)"#,
    )?;

    // 开机启动：默认开
    stmt.execute(params!["", pc, now, now, SYS_PLUGIN_ID, K_STARTUP, "1", "1"])?;
    // 主题：默认深色（"1" = 深色，"0" = 浅色）
    stmt.execute(params!["", pc, now, now, SYS_PLUGIN_ID, K_THEME, "1", "1"])?;
    Ok(())
}

/// 读取系统配置项的实际值（v_actual）
///
/// 若不存在则返回 None
pub fn get_sys_config(key: &str) -> Option<String> {
    let db = DB.get()?;
    let conn = db.lock().ok()?;
    let pc = pc_name();
    conn.query_row(
        r#"SELECT v_actual FROM configs
           WHERE userID = ? AND pcName = ? AND pluginID = ? AND k = ?"#,
        params!["", pc, SYS_PLUGIN_ID, key],
        |row| row.get::<_, Option<String>>(0),
    )
    .optional()
    .ok()
    .flatten()
    .flatten()
}

/// 写入/更新系统配置项（按 UNIQUE 约束 upsert）
///
/// 若不存在则插入（同时设置 v_default=value），否则只更新 v_actual + updatetime
pub fn set_sys_config(key: &str, value: &str) -> rusqlite::Result<()> {
    let Some(db) = DB.get() else {
        return Err(rusqlite::Error::InvalidQuery);
    };
    let conn = db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
    let pc = pc_name();
    let now = now_str();

    // 先尝试更新
    let updated = conn.execute(
        r#"UPDATE configs SET v_actual = ?, updatetime = ?, synced = 0
           WHERE userID = ? AND pcName = ? AND pluginID = ? AND k = ?"#,
        params![value, now, "", pc, SYS_PLUGIN_ID, key],
    )?;

    // 不存在则插入
    if updated == 0 {
        conn.execute(
            r#"INSERT INTO configs
               (userID, pcName, createtime, updatetime, pluginID, k, v_default, v_actual, synced)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0)"#,
            params!["", pc, now, now, SYS_PLUGIN_ID, key, value, value],
        )?;
    }
    Ok(())
}
