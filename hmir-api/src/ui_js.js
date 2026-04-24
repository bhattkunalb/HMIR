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
  downloadProgress: {}
};

// --- SSE Connections ---
function connectTelemetry() {
  const es = new EventSource(API + '/v1/telemetry');
  es.onmessage = e => {
    try {
      const d = JSON.parse(e.data);
      if (d.HardwareState) {
        state.telemetry = d.HardwareState;
        updateTelemetryUI();
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
  setText('cpu-temp', t.cpu_temp ? t.cpu_temp.toFixed(0) + '°C' : '--');
  setText('gpu-temp', t.gpu_temp ? t.gpu_temp.toFixed(0) + '°C' : '--');
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
  setInterval(checkHealth, 15000);
  document.getElementById('chat-input')?.addEventListener('keydown', e => {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendMessage(); }
  });
});
