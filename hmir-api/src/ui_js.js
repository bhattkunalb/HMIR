/* HMIR Web Console — Core Logic */
const API = location.origin;
const state = {
  telemetry: null,
  chatHistory: JSON.parse(localStorage.getItem('hmir_chat') || '[]'),
  logs: [],
  models: [],
  activeTab: 'overview',
  streaming: false,
  workerOnline: false,
  downloadProgress: {},
  telemetryHistory: []  // Ring buffer for sparklines (max 60)
};

// --- SSE Connections ---
function connectTelemetry() {
  const es = new EventSource(API + '/v1/telemetry');
  es.onmessage = e => {
    try {
      const d = JSON.parse(e.data);
      if (d.HardwareState) {
        state.telemetry = d.HardwareState;
        pushTelemetryHistory(d.HardwareState);
        updateTelemetryUI();
        if (state.activeTab === 'monitor') updateMonitorTab();
      } else if (d.DownloadStatus) {
        const prog = document.getElementById('dl-progress');
        if (prog) {
          prog.style.display = 'block';
          prog.querySelector('.download-info span:first-child').textContent = 'Downloading ' + d.DownloadStatus.model + '...';
          prog.querySelector('.download-info span:last-child').textContent = d.DownloadStatus.status;
          setProgress('dl-bar', d.DownloadStatus.progress * 100);
          if (d.DownloadStatus.progress >= 1.0) prog.style.display = 'none';
        }
      }
    } catch {}
  };
  es.onerror = () => { setTimeout(connectTelemetry, 3000); es.close(); };
}

function connectLogs() {
  const es = new EventSource(API + '/v1/logs');
  es.onmessage = e => {
    state.logs.push(e.data);
    if (state.logs.length > 500) state.logs.shift();
    if (state.activeTab === 'logs') renderLogs();
  };
  es.onerror = () => { setTimeout(connectLogs, 3000); es.close(); };
}

// --- Telemetry UI ---
function setGauge(id, pct) {
  const el = document.getElementById(id);
  if (!el) return;
  const circ = el.querySelector('.fg');
  const valEl = el.querySelector('.gauge-value');
  const r = 27, c = 2 * Math.PI * r;
  circ.setAttribute('stroke-dasharray', c);
  circ.setAttribute('stroke-dashoffset', c - (c * Math.min(pct, 100) / 100));
  if (valEl) valEl.textContent = Math.round(pct) + '%';
}

function updateTelemetryUI() {
  const t = state.telemetry;
  if (!t) return;
  setGauge('g-cpu', t.cpu_util || 0);
  setGauge('g-gpu', t.gpu_util || 0);
  setGauge('g-npu', t.npu_util || 0);
  setGauge('g-ram', t.ram_total > 0 ? (t.ram_used / t.ram_total * 100) : 0);

  setText('cpu-name', t.cpu_name || 'N/A');
  setText('gpu-name', t.gpu_name || 'N/A');
  setText('npu-name', t.npu_name || 'N/A');
  setText('cpu-temp', (t.cpu_temp !== undefined && t.cpu_temp !== null) ? t.cpu_temp.toFixed(0) + '°C' : '--');
  setText('gpu-temp', (t.gpu_temp !== undefined && t.gpu_temp !== null) ? t.gpu_temp.toFixed(0) + '°C' : '--');
  setText('ram-info', (t.ram_used||0).toFixed(1) + ' / ' + (t.ram_total||0).toFixed(1) + ' GB');
  setText('vram-info', (t.vram_used||0).toFixed(1) + ' / ' + (t.vram_total||0).toFixed(1) + ' GB');
  setText('disk-info', (t.disk_free||0).toFixed(0) + ' / ' + (t.disk_total||0).toFixed(0) + ' GB free');
  setText('tps-val', (t.tps||0).toFixed(1));
  setText('kv-val', (t.kv_cache||0).toFixed(1) + ' MB');
  setText('uptime-val', formatUptime(t.node_uptime || 0));
  setText('engine-status', t.engine_status || 'UNKNOWN');

  const badge = document.getElementById('sys-status');
  if (badge) {
    const online = t.engine_status && t.engine_status !== 'ERROR';
    badge.className = 'status-badge' + (online ? '' : ' offline');
    badge.querySelector('.status-text').textContent = online ? 'ONLINE' : 'OFFLINE';
  }

  // Progress bars
  setProgress('pb-ram', t.ram_total > 0 ? (t.ram_used / t.ram_total * 100) : 0);
  setProgress('pb-disk', t.disk_total > 0 ? ((t.disk_total - t.disk_free) / t.disk_total * 100) : 0);
}

function setText(id, val) { const el = document.getElementById(id); if (el) el.textContent = val; }
function setProgress(id, pct) {
  const el = document.getElementById(id);
  if (!el) return;
  el.style.width = Math.min(pct, 100) + '%';
  el.className = 'progress-fill' + (pct > 90 ? ' danger' : pct > 70 ? ' warn' : '');
}
function formatUptime(s) {
  const h = Math.floor(s/3600), m = Math.floor((s%3600)/60), sec = s%60;
  return `${h}h ${m}m ${sec}s`;
}

// --- Telemetry History ---
function pushTelemetryHistory(t) {
  state.telemetryHistory.push({
    cpu: t.cpu_util || 0,
    gpu: t.gpu_util || 0,
    npu: t.npu_util || 0,
    ts: Date.now()
  });
  if (state.telemetryHistory.length > 60) state.telemetryHistory.shift();
}

async function bootstrapHistory() {
  try {
    const r = await fetch(API + '/v1/hardware/history');
    const arr = await r.json();
    if (Array.isArray(arr)) {
      arr.forEach(item => {
        const hw = item.HardwareState || item;
        pushTelemetryHistory(hw);
      });
    }
  } catch {}
}

// --- Sparkline Drawing ---
function drawSparkline(canvasId, dataKey, colorHex) {
  const canvas = document.getElementById(canvasId);
  if (!canvas) return;
  const ctx = canvas.getContext('2d');
  const dpr = window.devicePixelRatio || 1;
  const rect = canvas.getBoundingClientRect();
  canvas.width = rect.width * dpr;
  canvas.height = rect.height * dpr;
  ctx.scale(dpr, dpr);
  const w = rect.width, h = rect.height;
  ctx.clearRect(0, 0, w, h);

  const data = state.telemetryHistory.map(p => p[dataKey]);
  if (data.length < 2) {
    // Draw flat line at 0
    ctx.strokeStyle = colorHex + '40';
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(0, h - 4);
    ctx.lineTo(w, h - 4);
    ctx.stroke();
    return;
  }

  const maxVal = Math.max(100, ...data);
  const stepX = w / (60 - 1);
  const offset = 60 - data.length;

  // Build points
  const points = data.map((v, i) => ({
    x: (offset + i) * stepX,
    y: h - 4 - ((v / maxVal) * (h - 8))
  }));

  // Gradient fill
  const grad = ctx.createLinearGradient(0, 0, 0, h);
  grad.addColorStop(0, colorHex + '30');
  grad.addColorStop(1, colorHex + '02');

  ctx.beginPath();
  ctx.moveTo(points[0].x, h);
  ctx.lineTo(points[0].x, points[0].y);
  for (let i = 1; i < points.length; i++) {
    const prev = points[i - 1];
    const curr = points[i];
    const cpx = (prev.x + curr.x) / 2;
    ctx.bezierCurveTo(cpx, prev.y, cpx, curr.y, curr.x, curr.y);
  }
  ctx.lineTo(points[points.length - 1].x, h);
  ctx.closePath();
  ctx.fillStyle = grad;
  ctx.fill();

  // Stroke line
  ctx.beginPath();
  ctx.moveTo(points[0].x, points[0].y);
  for (let i = 1; i < points.length; i++) {
    const prev = points[i - 1];
    const curr = points[i];
    const cpx = (prev.x + curr.x) / 2;
    ctx.bezierCurveTo(cpx, prev.y, cpx, curr.y, curr.x, curr.y);
  }
  ctx.strokeStyle = colorHex;
  ctx.lineWidth = 2;
  ctx.stroke();

  // Current value dot
  const last = points[points.length - 1];
  ctx.beginPath();
  ctx.arc(last.x, last.y, 3, 0, Math.PI * 2);
  ctx.fillStyle = colorHex;
  ctx.fill();
  ctx.beginPath();
  ctx.arc(last.x, last.y, 6, 0, Math.PI * 2);
  ctx.fillStyle = colorHex + '30';
  ctx.fill();

  // Grid lines
  ctx.strokeStyle = 'rgba(255,255,255,0.04)';
  ctx.lineWidth = 0.5;
  for (let i = 1; i < 4; i++) {
    const gy = (h / 4) * i;
    ctx.beginPath();
    ctx.moveTo(0, gy);
    ctx.lineTo(w, gy);
    ctx.stroke();
  }
}

// --- Monitor Tab Update ---
function updateMonitorTab() {
  const t = state.telemetry;
  if (!t) return;

  // Specs
  setText('mon-cpu-model', t.cpu_name || 'N/A');
  setText('mon-cpu-cores', t.cpu_cores || 'N/A');
  setText('mon-cpu-threads', t.cpu_threads || 'N/A');
  setText('mon-cpu-l3', t.cpu_l3_cache_mb ? t.cpu_l3_cache_mb.toFixed(1) + ' MB' : 'N/A');
  setText('mon-cpu-temp', formatTemp(t.cpu_temp));
  setText('mon-gpu-model', t.gpu_name || 'N/A');
  setText('mon-gpu-driver', t.gpu_driver || 'N/A');
  setText('mon-gpu-vram-ded', formatGB(t.gpu_vram_dedicated));
  setText('mon-gpu-vram-shr', formatGB(t.gpu_vram_shared));
  setText('mon-gpu-temp', formatTemp(t.gpu_temp));
  setText('mon-npu-model', t.npu_name || 'None');
  setText('mon-npu-driver', t.npu_driver || 'N/A');
  setText('mon-npu-vram', formatGB(t.npu_vram_used));
  setText('mon-npu-status', t.engine_status || 'N/A');
  setText('mon-uptime', formatUptime(t.node_uptime || 0));

  // Sparklines
  setText('mon-spark-cpu-val', Math.round(t.cpu_util || 0) + '%');
  setText('mon-spark-gpu-val', Math.round(t.gpu_util || 0) + '%');
  setText('mon-spark-npu-val', Math.round(t.npu_util || 0) + '%');
  drawSparkline('spark-cpu', 'cpu', '#06b6d4');
  drawSparkline('spark-gpu', 'gpu', '#8b5cf6');
  drawSparkline('spark-npu', 'npu', '#22c55e');

  // Memory bars
  const ramPct = t.ram_total > 0 ? (t.ram_used / t.ram_total * 100) : 0;
  setText('mon-ram-val', (t.ram_used||0).toFixed(1) + ' / ' + (t.ram_total||0).toFixed(1) + ' GB');
  setBarWidth('mon-ram-bar', ramPct);

  const vramTotal = (t.gpu_vram_dedicated||0) + (t.gpu_vram_shared||0);
  setText('mon-vram-ded-val', formatGB(t.gpu_vram_dedicated));
  setBarWidth('mon-vram-ded-bar', vramTotal > 0 ? ((t.gpu_vram_dedicated||0) / vramTotal * 100) : 0);
  setText('mon-vram-shr-val', formatGB(t.gpu_vram_shared));
  setBarWidth('mon-vram-shr-bar', vramTotal > 0 ? ((t.gpu_vram_shared||0) / vramTotal * 100) : 0);
  setText('mon-npu-mem-val', formatGB(t.npu_vram_used));
  setBarWidth('mon-npu-mem-bar', t.npu_vram_used > 0 ? Math.min((t.npu_vram_used / 1.0) * 100, 100) : 0);

  // Disk
  setText('mon-disk-model', t.disk_model || 'Disk');
  const diskUsed = (t.disk_total||0) - (t.disk_free||0);
  const diskPct = t.disk_total > 0 ? (diskUsed / t.disk_total * 100) : 0;
  setText('mon-disk-val', diskUsed.toFixed(0) + ' / ' + (t.disk_total||0).toFixed(0) + ' GB used');
  setBarWidth('mon-disk-bar', diskPct);
  setText('mon-ram-speed', t.ram_speed_mts ? t.ram_speed_mts + ' MT/s' : 'N/A');

  // Thermal
  updateThermalBadge('mon-cpu-temp-big', 'mon-cpu-thermal-badge', t.cpu_temp);
  updateThermalBadge('mon-gpu-temp-big', 'mon-gpu-thermal-badge', t.gpu_temp);
  setText('mon-power-val', t.power_w > 0 ? t.power_w.toFixed(1) + ' W' : 'N/A');

  // Engine
  setText('mon-engine-status', t.engine_status || 'N/A');
  setText('mon-tps', (t.tps||0).toFixed(1));
  setText('mon-kv', (t.kv_cache||0).toFixed(1) + ' MB');
  setText('mon-node-uptime', formatUptime(t.node_uptime || 0));
}

function formatGB(val) {
  if (!val || val === 0) return 'N/A';
  if (val < 1) return (val * 1024).toFixed(0) + ' MB';
  return val.toFixed(2) + ' GB';
}
function formatTemp(c) {
  if (!c || c === 0) return '--';
  return c.toFixed(0) + '°C';
}
function setBarWidth(id, pct) {
  const el = document.getElementById(id);
  if (el) el.style.width = Math.min(pct, 100) + '%';
}
function updateThermalBadge(tempId, badgeId, temp) {
  const tempEl = document.getElementById(tempId);
  const badgeEl = document.getElementById(badgeId);
  if (!tempEl || !badgeEl) return;
  if (!temp || temp === 0) {
    tempEl.textContent = '--';
    badgeEl.className = 'thermal-badge cool';
    badgeEl.textContent = 'N/A';
    return;
  }
  tempEl.textContent = temp.toFixed(0) + '°C';
  if (temp >= 80) {
    badgeEl.className = 'thermal-badge hot';
    badgeEl.textContent = 'HOT';
  } else if (temp >= 60) {
    badgeEl.className = 'thermal-badge warm';
    badgeEl.textContent = 'WARM';
  } else {
    badgeEl.className = 'thermal-badge cool';
    badgeEl.textContent = 'COOL';
  }
}

// --- Chat ---
function renderChat() {
  const box = document.getElementById('chat-messages');
  if (!box) return;
  box.innerHTML = state.chatHistory.map(m =>
    `<div class="chat-msg ${m.role}">
      <div>${escapeHtml(m.content)}</div>
      <div class="meta">${m.role === 'user' ? 'You' : 'HMIR NPU'} · ${m.time || ''}</div>
    </div>`
  ).join('');
  box.scrollTop = box.scrollHeight;
}

function saveChatHistory() {
  localStorage.setItem('hmir_chat', JSON.stringify(state.chatHistory.slice(-100)));
}

async function sendMessage() {
  const input = document.getElementById('chat-input');
  if (!input || !input.value.trim() || state.streaming) return;
  const msg = input.value.trim();
  input.value = '';
  const now = new Date().toLocaleTimeString();
  state.chatHistory.push({ role: 'user', content: msg, time: now });
  saveChatHistory();
  renderChat();
  state.streaming = true;
  updateSendBtn();
  showTyping(true);

  try {
    const resp = await fetch(API + '/v1/chat/completions', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ messages: [{ role: 'user', content: msg }], max_tokens: 512 })
    });
    showTyping(false);
    const reader = resp.body.getReader();
    const dec = new TextDecoder();
    let assistantMsg = { role: 'assistant', content: '', time: new Date().toLocaleTimeString() };
    state.chatHistory.push(assistantMsg);
    let buf = '';

    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      buf += dec.decode(value, { stream: true });
      const lines = buf.split('\n');
      buf = lines.pop() || '';
      for (const line of lines) {
        if (!line.startsWith('data: ') || line === 'data: [DONE]') continue;
        try {
          const j = JSON.parse(line.slice(6));
          if (j.choices?.[0]?.delta?.content) {
            assistantMsg.content += j.choices[0].delta.content;
            renderChat();
          }
          if (j.error) { assistantMsg.content += '\n[Error: ' + j.error + ']'; renderChat(); }
        } catch {}
      }
    }
    saveChatHistory();
  } catch (err) {
    showTyping(false);
    state.chatHistory.push({ role: 'assistant', content: '[Connection error: ' + err.message + ']', time: new Date().toLocaleTimeString() });
    saveChatHistory();
    renderChat();
  }
  state.streaming = false;
  updateSendBtn();
}

function showTyping(show) {
  const el = document.getElementById('typing-indicator');
  if (el) el.style.display = show ? 'flex' : 'none';
}
function updateSendBtn() {
  const btn = document.getElementById('send-btn');
  if (btn) btn.disabled = state.streaming;
}
function clearChat() {
  state.chatHistory = [];
  saveChatHistory();
  renderChat();
}

// --- Models ---
async function loadModels() {
  try {
    const r = await fetch(API + '/v1/models/installed');
    const data = await r.json();
    state.models = data.models || [];
    renderModels();
  } catch { state.models = []; renderModels(); }
}

function renderModels() {
  const box = document.getElementById('model-list');
  if (!box) return;
  if (state.models.length === 0) {
    box.innerHTML = '<div style="padding:20px;text-align:center;color:var(--text-dim)">No models installed. Download one below.</div>';
    return;
  }
  box.innerHTML = state.models.map(m =>
    `<div class="model-row">
      <div><div class="model-name">${escapeHtml(m.name || m)}</div>
      <div class="model-size">${m.size || ''}</div></div>
      <div style="display:flex;gap:8px;align-items:center">
        <span class="model-badge ${m.active ? 'active' : 'idle'}">${m.active ? 'ACTIVE' : 'IDLE'}</span>
        ${!m.active ? `<button class="btn btn-primary" onclick="switchModel('${escapeHtml(m.name||m)}')">Load</button>` : ''}
        ${!m.active ? `<button class="btn btn-danger" onclick="ejectModel('${escapeHtml(m.name||m)}')">Eject</button>` : ''}
      </div>
    </div>`
  ).join('');
}

async function switchModel(name) {
  try {
    setText('engine-status', 'LOADING...');
    await fetch(API + '/v1/engine/switch', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name })
    });
    await loadModels();
  } catch (e) { alert('Failed to switch model: ' + e.message); }
}

async function ejectModel(name) {
  if (!confirm('Eject model ' + name + '?')) return;
  try {
    await fetch(API + '/v1/engine/eject', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name })
    });
    await loadModels();
  } catch (e) { alert('Failed: ' + e.message); }
}

async function downloadModel() {
  const input = document.getElementById('dl-model-name');
  if (!input || !input.value.trim()) return;
  const name = input.value.trim();
  input.value = '';
  const prog = document.getElementById('dl-progress');
  if (prog) {
    prog.style.display = 'block';
    prog.querySelector('.download-info span:first-child').textContent = 'Downloading ' + name + '...';
  }
  try {
    const r = await fetch(API + '/v1/models/download', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name })
    });
    const data = await r.json();
    if (data.error) { alert('Download failed: ' + data.error); }
    else { alert('Download complete: ' + name); await loadModels(); }
  } catch (e) { alert('Download error: ' + e.message); }
  if (prog) prog.style.display = 'none';
}

// --- Logs ---
function renderLogs() {
  const box = document.getElementById('log-viewer');
  if (!box) return;
  const filter = (document.getElementById('log-filter')?.value || '').toLowerCase();
  const filtered = filter ? state.logs.filter(l => l.toLowerCase().includes(filter)) : state.logs;
  box.innerHTML = filtered.slice(-200).map(l => {
    let cls = 'lvl-info';
    if (l.includes('WARN') || l.includes('⚠')) cls = 'lvl-warn';
    if (l.includes('ERROR') || l.includes('CRITICAL')) cls = 'lvl-err';
    return `<div class="log-line"><span class="${cls}">${escapeHtml(l)}</span></div>`;
  }).join('');
  box.scrollTop = box.scrollHeight;
}

// --- Navigation ---
function switchTab(tab) {
  state.activeTab = tab;
  document.querySelectorAll('.nav-tab').forEach(t => t.classList.toggle('active', t.dataset.tab === tab));
  document.querySelectorAll('.tab-content').forEach(p => p.style.display = p.id === 'tab-' + tab ? 'block' : 'none');
  if (tab === 'models') loadModels();
  if (tab === 'logs') renderLogs();
  if (tab === 'chat') renderChat();
  if (tab === 'monitor') updateMonitorTab();
}

// --- Helpers ---
function escapeHtml(s) {
  return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
}

// --- Health check ---
async function checkHealth() {
  try {
    const r = await fetch(API + '/v1/health', { signal: AbortSignal.timeout(3000) });
    const d = await r.json();
    state.workerOnline = d.npu_worker === 'READY';
  } catch { state.workerOnline = false; }
}

// --- Init ---
window.addEventListener('DOMContentLoaded', () => {
  connectTelemetry();
  connectLogs();
  renderChat();
  loadModels();
  checkHealth();
  bootstrapHistory();
  setInterval(checkHealth, 15000);
  document.getElementById('chat-input')?.addEventListener('keydown', e => {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendMessage(); }
  });
});
