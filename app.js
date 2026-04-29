// ── Imports ────────────────────────────────────────────────────────
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

// ── State ─────────────────────────────────────────────────────────
let currentDatabases = [];
let executing = false;
let logCount = 0;
const MAX_LOG = 1000;

// Active event unlisten functions (cleaned up between executions)
let unlistenExecResult = null;
let unlistenExecComplete = null;

// ── Init ──────────────────────────────────────────────────────────
window.addEventListener('DOMContentLoaded', () => {
  refreshConfig();

  // Wire up event handlers
  document.getElementById('btnMatch').addEventListener('click', matchDatabases);
  document.getElementById('btnExecute').addEventListener('click', executeSQL);
  document.getElementById('btnClearLog').addEventListener('click', clearLogs);
  document.getElementById('btnRefresh').addEventListener('click', refreshConfig);
  document.getElementById('btnConfigDir').addEventListener('click', openConfigDir);
  document.getElementById('btnToggleDbList').addEventListener('click', toggleDbList);
  document.getElementById('checkAll').addEventListener('change', toggleAll);

  // Ctrl+Enter shortcut in SQL editor
  document.getElementById('sqlEditor').addEventListener('keydown', (e) => {
    if (e.ctrlKey && e.key === 'Enter') {
      e.preventDefault();
      executeSQL();
    }
  });
});

// ── Config ────────────────────────────────────────────────────────
async function refreshConfig() {
  try {
    const connections = await invoke('list_connections');
    const sel = document.getElementById('connectionSelect');

    // Note: config.ini is always found at the exe directory or CWD
    document.getElementById('configPath').textContent = 'config.ini';

    sel.innerHTML = '';
    if (connections && connections.length > 0) {
      connections.forEach(c => {
        const opt = document.createElement('option');
        opt.value = c.name;
        opt.textContent = `${c.name} (${c.host}:${c.port})`;
        sel.appendChild(opt);
      });
      updateConnStatus('online');
      showToast(`已加载 ${connections.length} 个连接配置`, 'success');
    } else {
      const opt = document.createElement('option');
      opt.value = '';
      opt.textContent = '— 无配置 —';
      sel.appendChild(opt);
      updateConnStatus('offline');
      showToast('未找到数据库配置', 'error');
    }
  } catch (err) {
    updateConnStatus('offline');
    showToast('配置读取失败: ' + (typeof err === 'string' ? err : err.message || '未知错误'), 'error');
  }
}

function updateConnStatus(status) {
  const sel = document.getElementById('connectionSelect');
  if (status === 'online') {
    sel.style.borderLeft = '3px solid var(--success)';
  } else {
    sel.style.borderLeft = '3px solid var(--text-muted)';
  }
}

function openConfigDir() {
  // Tauri doesn't have window.open for filesystem access.
  // Show the config location information to the user.
  showToast('配置文件 config.ini 位于应用程序所在目录，用文本编辑器即可编辑', 'info');
}

// ── Database Matching ─────────────────────────────────────────────
async function matchDatabases() {
  const connName = document.getElementById('connectionSelect').value;
  const prefix = document.getElementById('prefixInput').value.trim();
  const suffix = document.getElementById('suffixInput').value.trim();

  if (!connName) {
    showToast('请先选择连接实例', 'error');
    return;
  }

  const btn = document.getElementById('btnMatch');
  btn.disabled = true;
  btn.textContent = '⏳ 查询中...';

  try {
    const databases = await invoke('match_databases', {
      connectionName: connName,  // camelCase — Tauri serde renames snake_case automatically
      prefix: prefix,
      suffix: suffix,
    });

    currentDatabases = databases || [];
    renderDatabases(currentDatabases);
    document.getElementById('matchCount').innerHTML = `(<strong>${currentDatabases.length}</strong> 个)`;

    if (currentDatabases.length === 0) {
      showToast('未匹配到任何数据库', 'info');
    } else {
      showToast(`匹配到 ${currentDatabases.length} 个数据库`, 'success');
    }
  } catch (err) {
    showToast('查询失败: ' + (typeof err === 'string' ? err : err.message || '未知错误'), 'error');
    currentDatabases = [];
    renderDatabases([]);
  } finally {
    btn.disabled = false;
    btn.textContent = '🔍 匹配数据库';
  }
}

function renderDatabases(dbs) {
  const body = document.getElementById('dbListBody');
  if (dbs.length === 0) {
    body.innerHTML = '<div class="db-empty">未匹配到数据库</div>';
    document.getElementById('checkAll').checked = true;
    return;
  }

  body.innerHTML = dbs.map((db, i) => `
    <div class="db-item">
      <input type="checkbox" id="db_${i}" checked onchange="window.__updateCheckAll()">
      <span class="db-icon">🗄️</span>
      <label for="db_${i}" style="cursor:pointer;flex:1;">${escapeHtml(db)}</label>
    </div>
  `).join('');
  document.getElementById('checkAll').checked = true;
}

// ── Checkbox Helpers ──────────────────────────────────────────────
function toggleAll() {
  const checked = document.getElementById('checkAll').checked;
  document.querySelectorAll('#dbListBody input[type="checkbox"]').forEach(cb => cb.checked = checked);
}

function updateCheckAll() {
  const all = document.querySelectorAll('#dbListBody input[type="checkbox"]');
  const checked = document.querySelectorAll('#dbListBody input[type="checkbox"]:checked');
  document.getElementById('checkAll').checked = all.length > 0 && all.length === checked.length;
}

// Expose for inline onchange handlers
window.__updateCheckAll = updateCheckAll;

let dbListCollapsed = false;
function toggleDbList() {
  const body = document.getElementById('dbListBody');
  const icon = document.getElementById('dbToggleIcon');
  dbListCollapsed = !dbListCollapsed;
  if (dbListCollapsed) {
    body.classList.add('collapsed');
    icon.textContent = '▼';
  } else {
    body.classList.remove('collapsed');
    icon.textContent = '▲';
  }
}

// ── SQL Execution ─────────────────────────────────────────────────
async function executeSQL() {
  if (executing) return;

  const connName = document.getElementById('connectionSelect').value;
  const sql = document.getElementById('sqlEditor').value.trim();

  if (!connName) {
    showToast('请先选择连接实例', 'error');
    return;
  }
  if (!sql) {
    showToast('请输入 SQL 脚本', 'error');
    return;
  }

  // Get checked databases
  const checkedItems = document.querySelectorAll('#dbListBody input[type="checkbox"]:checked');
  if (checkedItems.length === 0) {
    showToast('请至少勾选一个数据库', 'error');
    return;
  }

  const databases = Array.from(checkedItems).map(cb => {
    const idx = parseInt(cb.id.replace('db_', ''));
    return currentDatabases[idx];
  });

  // Clean up any previous listeners
  await cleanupListeners();

  // Lock UI
  executing = true;
  document.getElementById('btnExecute').disabled = true;
  document.getElementById('btnExecute').textContent = '⏳ 执行中...';
  document.getElementById('sqlEditor').disabled = true;
  document.getElementById('successCount').textContent = '0';
  document.getElementById('failCount').textContent = '0';
  clearLogs();

  addLog('info', '', '🚀 开始执行...', '');

  // Set up event listeners BEFORE invoking execute_sql
  // This ensures we don't miss any events
  unlistenExecResult = await listen('exec-result', (event) => {
    const payload = event.payload;
    if (payload && payload.result) {
      const r = payload.result;
      const statusClass = r.success ? 'success' : 'error';
      const statusText = r.success ? '✅ 成功' : '❌ 失败';
      const duration = r.duration != null ? ` (${Number(r.duration).toFixed(2)}s)` : '';
      const msg = r.success ? '执行成功' : (r.error || '未知错误');
      addLog(statusClass, r.database, statusText, `${msg}${duration}`);

      if (r.success) {
        document.getElementById('successCount').textContent =
          parseInt(document.getElementById('successCount').textContent) + 1;
      } else {
        document.getElementById('failCount').textContent =
          parseInt(document.getElementById('failCount').textContent) + 1;
      }
    }
    if (payload && payload.progress) {
      document.getElementById('execProgress').style.display = 'inline';
      document.getElementById('execProgress').textContent = `进度: ${payload.progress}`;
    }
  });

  unlistenExecComplete = await listen('exec-complete', async (event) => {
    const payload = event.payload;
    if (payload && payload.summary) {
      const s = payload.summary;
      addLog('summary', '', '🏁 执行完成',
        `共 ${s.total} 个库，成功 ${s.success_count}，失败 ${s.fail_count}`);
    }

    // Unlock UI
    executing = false;
    document.getElementById('btnExecute').disabled = false;
    document.getElementById('btnExecute').textContent = '▶ 同步执行';
    document.getElementById('sqlEditor').disabled = false;
    document.getElementById('execProgress').style.display = 'none';

    // Clean up listeners
    await cleanupListeners();
  });

  try {
    // Fire-and-forget: execute_sql returns void, results come via events
    await invoke('execute_sql', {
      connectionName: connName,
      databases: databases,
      sql: sql,
    });
  } catch (err) {
    addLog('error', '', '❌ 执行启动失败', typeof err === 'string' ? err : err.message || '未知错误');

    // Unlock UI on invoke failure
    executing = false;
    document.getElementById('btnExecute').disabled = false;
    document.getElementById('btnExecute').textContent = '▶ 同步执行';
    document.getElementById('sqlEditor').disabled = false;
    document.getElementById('execProgress').style.display = 'none';

    await cleanupListeners();
  }
}

async function cleanupListeners() {
  if (unlistenExecResult) {
    try { unlistenExecResult(); } catch (_) { /* ignore */ }
    unlistenExecResult = null;
  }
  if (unlistenExecComplete) {
    try { unlistenExecComplete(); } catch (_) { /* ignore */ }
    unlistenExecComplete = null;
  }
}

// ── Log ───────────────────────────────────────────────────────────
function addLog(type, db, status, msg) {
  const body = document.getElementById('logBody');

  // Remove empty placeholder
  const empty = body.querySelector('.log-empty');
  if (empty) empty.remove();

  const now = new Date();
  const time = now.toLocaleTimeString('zh-CN', { hour12: false });

  const entry = document.createElement('div');
  entry.className = `log-entry ${type}`;
  entry.innerHTML = `
    <span class="log-time">${time}</span>
    <span class="log-db">${db ? escapeHtml(db) : ''}</span>
    <span class="log-status">${status}</span>
    <span class="log-msg">${escapeHtml(msg)}</span>
  `;
  body.appendChild(entry);

  logCount++;
  if (logCount > MAX_LOG) {
    const first = body.querySelector('.log-entry');
    if (first) first.remove();
    logCount--;
  }

  body.scrollTop = body.scrollHeight;
}

function clearLogs() {
  const body = document.getElementById('logBody');
  body.innerHTML = '<div class="log-empty">等待执行...</div>';
  document.getElementById('successCount').textContent = '0';
  document.getElementById('failCount').textContent = '0';
  logCount = 0;
}

// ── Toast ─────────────────────────────────────────────────────────
function showToast(msg, type) {
  const container = document.getElementById('toastContainer');
  const toast = document.createElement('div');
  toast.className = `toast ${type}`;
  toast.textContent = msg;
  container.appendChild(toast);
  setTimeout(() => { toast.remove(); }, 3500);
}

// ── Utilities ─────────────────────────────────────────────────────
function escapeHtml(str) {
  const div = document.createElement('div');
  div.textContent = str;
  return div.innerHTML;
}
