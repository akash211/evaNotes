/**
 * evaNotes - SPA controller.
 *
 * Backend contract (must stay in sync with src/handlers):
 *  - GET  /api/auth/status  -> { authenticated, google_login_enabled, google_client_id,
 *                                 telegram_enabled, email_enabled, ai_enabled, ai_models,
 *                                 ai_default_model, ai_styles:[{id,label}], version }
 *  - GET  /api/auth/me      -> { status, user_id, email, is_admin }
 *  - POST /api/auth/otp/request { email }
 *  - POST /api/auth/otp/verify  { email, code } -> flat { status, email, is_admin, user_id, token }
 *  - POST /api/auth/google      { id_token }    -> flat
 *  - POST /api/auth/logout
 *  - GET  /api/notes?page&limit&search&search_in&type&archived&pinned&tag=a,b&folder -> { notes, pagination }
 *  - POST /api/notes  { title, content, note_type, color, icon, folder_id, tags[] }
 *  - GET  /api/notes/{id}?device= -> { note, content_html, active_editors:[{device,since}] }
 *  - PUT  /api/notes/{id}  partial; folder_id tri-state (absent/null/id)
 *  - DELETE /api/notes/{id} ; PUT /api/notes/{id}/pin|archive|tags
 *  - GET  /api/notes/{id}/versions -> { versions:[{id,title,content,note_type,created_at}] }
 *  - POST /api/notes/{id}/versions/clear ; POST /api/notes/{id}/restore { version_id }
 *  - GET/POST/DELETE /api/notes/{id}/share ; POST { mode: snapshot|live }
 *  - POST/DELETE /api/notes/{id}/presence { device }
 *  - POST /api/highlight { language, code } -> { html }
 *  - POST /api/ai/rewrite { text, model, style } -> { rewritten }
 *  - GET  /api/folders -> { folders: tree } ; POST {name,parent_id,icon} ; PUT/DELETE /api/folders/{id}
 *  - GET  /api/tags -> { tags:[{id,name,color,note_count}] } ; GET /api/tags/suggest?q=
 *  - GET/POST /api/reminders ; PUT/DELETE /api/reminders/{id}
 *  - POST /api/reminders/{id}/snooze {duration_minutes} ; /acknowledge {action}
 *  - WS   /ws -> { type:"reminder_triggered", data: reminder }
 *  - Public: /share/{slug} page, /share/{slug}/md, /share/{slug}/pdf
 */

(function () {
  'use strict';

  // ---------------------------------------------------------------- state
  const state = {
    user: null,
    view: 'notes',                    // notes | calendar | reminders | archive
    noteFilter: 'all',                // all | simple | detailed | code | sheet | pinned
    reminderFilter: 'all',
    searchQuery: '',
    searchIn: { title: true, content: true, tags: true, folders: true },
    tagFilters: [],                   // selected tag names (AND)
    folderFilter: null,               // { id, name, path } or null
    page: 1,
    pageSize: 24,
    totalNotes: 0,
    notes: [],
    reminders: [],
    tags: [],
    folderTree: [],
    capabilities: { google: false, telegram: false, email: false, ai: false, aiModels: [], aiDefaultModel: '', aiStyles: [] },
    ai: { enabled: false, model: '', style: 'minimal' },
    calendar: { mode: 'month', cursor: new Date() },
    deviceId: localStorage.getItem('evanotes_device_id') || (() => {
      const d = 'dev-' + Math.random().toString(36).slice(2, 10) + Date.now().toString(36);
      localStorage.setItem('evanotes_device_id', d);
      return d;
    })(),
    editor: {
      note: null,
      type: 'simple',
      color: '',
      icon: '',
      tags: [],
      folderId: '',
      pinned: false,
      archived: false,
      lang: 'text',
      sheet: null,                    // parsed rows while editing sheet notes
      dirty: false,
      saving: false,
      presenceTimer: null,
      heartbeatTimer: null,
    },
    reminderEditor: { reminder: null },
    folderEditor: { folder: null, icon: '' },
    shareState: { noteId: null, share: null },
    ws: null,
    wsRetry: null,
    wsAttempts: 0,
  };

  // ---------------------------------------------------------------- utils
  const $ = (sel) => document.querySelector(sel);
  const $$ = (sel) => Array.from(document.querySelectorAll(sel));

  function escapeHtml(str) {
    if (str == null) return '';
    return String(str)
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;')
      .replace(/'/g, '&#039;');
  }

  function escapeRegExp(s) { return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'); }

  function debounce(fn, ms) {
    let t = null;
    return (...args) => { clearTimeout(t); t = setTimeout(() => fn(...args), ms); };
  }

  function formatDate(isoStr) {
    if (!isoStr) return '';
    try {
      const d = new Date(isoStr);
      if (Number.isNaN(d.getTime())) return isoStr;
      const now = new Date();
      const diffDays = Math.floor((now - d) / 86400000);
      const time = d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
      if (diffDays === 0) return `Today, ${time}`;
      if (diffDays === 1) return `Yesterday, ${time}`;
      if (diffDays < 7) return `${d.toLocaleDateString([], { weekday: 'short' })}, ${time}`;
      return d.toLocaleDateString([], { month: 'short', day: 'numeric', year: d.getFullYear() !== now.getFullYear() ? 'numeric' : undefined });
    } catch (_) { return isoStr; }
  }

  function formatDateTimeLocal(dateObj) {
    const pad = (n) => String(n).padStart(2, '0');
    return `${dateObj.getFullYear()}-${pad(dateObj.getMonth() + 1)}-${pad(dateObj.getDate())}T${pad(dateObj.getHours())}:${pad(dateObj.getMinutes())}`;
  }

  function canonicalToLocalInput(canonical) {
    const d = new Date(canonical);
    if (Number.isNaN(d.getTime())) return '';
    return formatDateTimeLocal(d);
  }

  function localInputToIso(localValue) {
    const d = new Date(localValue);
    if (Number.isNaN(d.getTime())) return null;
    return d.toISOString();
  }

  function highlight(text, query) {
    const safe = escapeHtml(text);
    if (!query || !query.trim()) return safe;
    const terms = query.trim().split(/\s+/).filter(Boolean).slice(0, 5);
    let out = safe;
    for (const term of terms) {
      out = out.replace(new RegExp(`(${escapeRegExp(escapeHtml(term))})`, 'gi'), '<mark>$1</mark>');
    }
    return out;
  }

  function stripMarkdown(text) {
    return (text || '')
      .replace(/```[\s\S]*?```/g, ' ')
      .replace(/`([^`\n]+)`/g, '$1')
      .replace(/!\[[^\]]*\]\([^)]*\)/g, ' ')
      .replace(/\[([^\]]+)\]\([^)]*\)/g, '$1')
      .replace(/^#{1,6}\s+/gm, '')
      .replace(/(\*\*|__)(.*?)\1/g, '$2')
      .replace(/(\*|_)(.*?)\1/g, '$2')
      .replace(/^\s*[-*+]\s+/gm, '• ')
      .replace(/^\s*\d+\.\s+/gm, '')
      .replace(/^>\s?/gm, '')
      .replace(/\s+/g, ' ')
      .trim();
  }

  function noteSnippet(note, maxLen) {
    if (note.note_type === 'sheet') {
      try {
        const rows = JSON.parse(note.content || '[]');
        return rows.slice(0, 2).map((r) => r.slice(0, 4).join(' · ')).join('  |  ').slice(0, maxLen);
      } catch (_) { return note.content.slice(0, maxLen); }
    }
    const flat = stripMarkdown(note.content);
    return flat.length > maxLen ? flat.slice(0, maxLen).trimEnd() + '…' : flat;
  }

  // Safe client-side Markdown rendering for live preview
  function renderMarkdownClient(md) {
    if (!md) return '';
    let safe = escapeHtml(md);

    safe = safe.replace(/```([a-zA-Z0-9_+-]*)\n([\s\S]*?)```/g, (_, lang, code) => {
      if (lang.toLowerCase() === 'mermaid') {
        return `<div class="mermaid-block">${code.trim()}</div>`;
      }
      return `<pre><code>${code.trim()}</code></pre>`;
    });
    safe = safe.replace(/`([^`\n]+)`/g, '<code>$1</code>');
    safe = safe.replace(/^### (.*$)/gim, '<h3>$1</h3>');
    safe = safe.replace(/^## (.*$)/gim, '<h2>$1</h2>');
    safe = safe.replace(/^# (.*$)/gim, '<h1>$1</h1>');
    safe = safe.replace(/^&gt; (.*$)/gim, '<blockquote>$1</blockquote>');
    safe = safe.replace(/^\s*[-*] \[ \]\s+(.*$)/gim, '<li class="task-item"><input type="checkbox" disabled> $1</li>');
    safe = safe.replace(/^\s*[-*] \[x\]\s+(.*$)/gim, '<li class="task-item"><input type="checkbox" checked disabled> $1</li>');
    safe = safe.replace(/^\s*[-*] \s+(.*$)/gim, '<li>$1</li>');
    safe = safe.replace(/^\s*\d+\. \s+(.*$)/gim, '<li>$1</li>');
    safe = safe.replace(/\*\*(.*?)\*\*/g, '<strong>$1</strong>');
    safe = safe.replace(/__(.*?)__/g, '<strong>$1</strong>');
    safe = safe.replace(/\*(.*?)\*/g, '<em>$1</em>');
    safe = safe.replace(/_([^_]+)_/g, '<em>$1</em>');
    safe = safe.replace(/\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/g, '<a href="$2" target="_blank" rel="noopener noreferrer">$1</a>');
    safe = safe.replace(/^---$/gim, '<hr>');

    const paragraphs = safe.split(/\n{2,}/);
    return paragraphs
      .map((p) => {
        const trimmed = p.trim();
        if (!trimmed) return '';
        if (/^<(h1|h2|h3|pre|blockquote|li|hr|div)/.test(trimmed)) return trimmed;
        return `<p>${trimmed.replace(/\n/g, '<br>')}</p>`;
      })
      .join('\n');
  }

  // Mermaid: lazy-load from CDN and render all blocks in the preview pane
  let mermaidLoaded = false;
  let mermaidLoading = false;

  async function renderMermaidBlocks(container) {
    const blocks = $$('.mermaid-block', container);
    if (!blocks.length) return;
    if (!mermaidLoaded) {
      if (mermaidLoading) return;
      mermaidLoading = true;
      try {
        await new Promise((resolve, reject) => {
          const script = document.createElement('script');
          script.src = 'https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.min.js';
          script.onload = () => { if (window.mermaid) window.mermaid.initialize({ startOnLoad: false, theme: 'dark' }); resolve(); };
          script.onerror = () => reject(new Error('mermaid load failed'));
          document.head.appendChild(script);
        });
        mermaidLoaded = true;
      } catch (_) { mermaidLoading = false; return; }
      mermaidLoading = false;
    }
    for (const block of blocks) {
      const id = 'mmd-' + Math.random().toString(36).slice(2, 9);
      try {
        const { svg } = await window.mermaid.render(id, block.textContent.trim());
        block.outerHTML = `<div class="mermaid-rendered">${svg}</div>`;
      } catch (_) { /* invalid diagram: leave as-is */ }
    }
  }

  // ---------------------------------------------------------------- api
  async function api(path, options = {}) {
    options.headers = options.headers || {};
    if (options.body != null && typeof options.body === 'object' && !(options.body instanceof FormData)) {
      options.headers['Content-Type'] = 'application/json';
      options.body = JSON.stringify(options.body);
    }
    options.credentials = 'same-origin';
    return fetch(path, options);
  }

  async function apiJson(path, options = {}) {
    const res = await api(path, options);
    let data = null;
    try { data = await res.json(); } catch (_) { data = null; }
    if (res.status === 401) {
      showAuthModal();
      throw new Error((data && data.message) || 'Session expired — please sign in again.');
    }
    if (!res.ok) {
      throw new Error((data && data.message) || (data && data.error) || `Request failed (${res.status})`);
    }
    return data;
  }

  // ---------------------------------------------------------------- toasts
  function showToast(kind, title, message, actionsHtml) {
    const container = $('#toast-container');
    const toast = document.createElement('div');
    toast.className = `toast toast-${kind}`;
    toast.innerHTML = `
      <div class="toast-title">${escapeHtml(title)}</div>
      ${message ? `<div class="toast-body">${escapeHtml(message)}</div>` : ''}
      ${actionsHtml ? `<div class="toast-actions">${actionsHtml}</div>` : ''}
    `;
    container.appendChild(toast);
    const kill = () => { toast.classList.add('toast-out'); setTimeout(() => toast.remove(), 250); };
    setTimeout(kill, kind === 'reminder' ? 20000 : 4000);
    return toast;
  }

  function showReminderToast(reminder) {
    const toast = showToast(
      'reminder',
      `⏰ ${reminder.title || 'Reminder'}`,
      reminder.description || 'It is time for your scheduled reminder!',
      `<button class="btn btn-glass btn-sm" data-act="snooze">Snooze 10m</button>
       <button class="btn btn-primary btn-sm" data-act="done">Done</button>`
    );
    const snooze = toast.querySelector('[data-act="snooze"]');
    const done = toast.querySelector('[data-act="done"]');
    if (snooze) snooze.onclick = async () => { await snoozeReminder(reminder.id, 10); toast.remove(); };
    if (done) done.onclick = async () => { await acknowledgeReminder(reminder.id); toast.remove(); };

    if ('Notification' in window && Notification.permission === 'granted') {
      try { new Notification(`evaNotes: ${reminder.title || 'Reminder'}`, { body: reminder.description || 'Your reminder is due.' }); } catch (_) {}
    }
  }

  function confirmDialog(message) {
    return new Promise((resolve) => {
      $('#confirm-message').textContent = message;
      $('#modal-confirm').classList.remove('hidden');
      const cleanup = (result) => {
        $('#modal-confirm').classList.add('hidden');
        $('#btn-confirm-ok').onclick = null;
        $('#btn-confirm-cancel').onclick = null;
        resolve(result);
      };
      $('#btn-confirm-ok').onclick = () => cleanup(true);
      $('#btn-confirm-cancel').onclick = () => cleanup(false);
    });
  }

  // ---------------------------------------------------------------- auth
  async function checkAuthStatus() {
    try {
      const data = await apiJson('/api/auth/status');
      state.capabilities.google = !!data.google_login_enabled;
      state.capabilities.telegram = !!data.telegram_enabled;
      state.capabilities.email = !!data.email_enabled;
      state.capabilities.ai = !!data.ai_enabled;
      state.capabilities.aiModels = data.ai_models || [];
      state.capabilities.aiDefaultModel = data.ai_default_model || '';
      state.capabilities.aiStyles = data.ai_styles || [];
      state.googleClientId = data.google_client_id || null;
      $('#version-tag').textContent = `v${data.version || '0.1.0'}`;

      // AI model resets every session (page load) to the server default
      state.ai.model = state.capabilities.aiDefaultModel;

      renderAiHeader();
      renderAiEditorGroup();

      if (data.authenticated) await fetchCurrentUser();
      else showAuthModal();
    } catch (err) {
      console.error('Auth check failed:', err);
      showAuthModal();
    }
  }

  async function fetchCurrentUser() {
    try {
      const user = await apiJson('/api/auth/me');
      onLoginSuccess(user);
    } catch (_) { showAuthModal(); }
  }

  function onLoginSuccess(user) {
    state.user = user;
    hideAuthModal();
    $('#user-email-display').textContent = user.email || '—';
    $('#user-role-display').textContent = user.is_admin ? 'Administrator' : 'User';
    $('#user-avatar-initials').textContent = ((user.email || '?')[0] || '?').toUpperCase();
    initWebSocket();
    refreshAll();
    if ('Notification' in window && Notification.permission === 'default') {
      Notification.requestPermission().catch(() => {});
    }
  }

  function showAuthModal() {
    $('#modal-auth').classList.remove('hidden');
    $('#otp-step-1').classList.remove('hidden');
    $('#otp-step-2').classList.add('hidden');
    hideAuthError();
    renderGoogleButton();
  }

  function hideAuthModal() { $('#modal-auth').classList.add('hidden'); }
  function showAuthError(msg) { const b = $('#auth-error-msg'); b.textContent = msg; b.classList.remove('hidden'); }
  function hideAuthError() { $('#auth-error-msg').classList.add('hidden'); }

  async function requestOtp() {
    const email = $('#auth-email-input').value.trim();
    if (!email || !email.includes('@')) { showAuthError('Please enter a valid email address.'); return; }
    const btn = $('#btn-request-otp');
    btn.disabled = true;
    btn.textContent = 'Sending…';
    hideAuthError();
    try {
      const res = await api('/api/auth/otp/request', { method: 'POST', body: { email } });
      if (res.status === 429) {
        const data = await res.json().catch(() => null);
        const retryAfter = Math.max(1, parseInt(res.headers.get('Retry-After') || '60', 10) || 60);
        showAuthError((data && data.message) || 'Too many requests — please wait a moment.');
        startResendCountdown(btn, retryAfter);
        return;
      }
      const data = await res.json();
      if (!res.ok) { showAuthError((data && data.message) || 'Failed to request the code.'); return; }
      $('#otp-step-1').classList.add('hidden');
      $('#otp-step-2').classList.remove('hidden');
      $('#auth-code-input').value = '';
      $('#auth-code-input').focus();
    } catch (err) {
      showAuthError(err.message || 'Network error — is the server reachable?');
    } finally {
      if (!btn.dataset.counting) { btn.disabled = false; btn.textContent = 'Send Verification Code'; }
    }
  }

  function startResendCountdown(btn, seconds) {
    btn.dataset.counting = '1';
    btn.disabled = true;
    let left = seconds;
    btn.textContent = `Try again in ${left}s`;
    const timer = setInterval(() => {
      left -= 1;
      if (left <= 0) {
        clearInterval(timer);
        delete btn.dataset.counting;
        btn.disabled = false;
        btn.textContent = 'Send Verification Code';
        hideAuthError();
      } else {
        btn.textContent = `Try again in ${left}s`;
      }
    }, 1000);
  }

  async function verifyOtp() {
    const code = $('#auth-code-input').value.trim();
    if (!/^\d{6}$/.test(code)) { showAuthError('Please enter the 6-digit code.'); return; }
    const btn = $('#btn-verify-otp');
    btn.disabled = true;
    btn.textContent = 'Verifying…';
    hideAuthError();
    try {
      const data = await apiJson('/api/auth/otp/verify', {
        method: 'POST',
        body: { email: $('#auth-email-input').value.trim(), code },
      });
      onLoginSuccess({ email: data.email, is_admin: !!data.is_admin, user_id: data.user_id });
    } catch (err) {
      showAuthError(err.message);
    } finally {
      btn.disabled = false;
      btn.textContent = 'Sign In';
    }
  }

  function renderGoogleButton() {
    const container = $('#google-auth-container');
    const btnContainer = $('#google-btn-container');
    if (!state.capabilities.google || !state.googleClientId) { container.classList.add('hidden'); return; }
    container.classList.remove('hidden');

    window.handleGoogleCredentialResponse = async (response) => {
      try {
        const data = await apiJson('/api/auth/google', { method: 'POST', body: { id_token: response.credential } });
        onLoginSuccess({ email: data.email, is_admin: !!data.is_admin, user_id: data.user_id });
      } catch (err) {
        showAuthError(err.message || 'Google sign-in failed.');
      }
    };

    const init = () => {
      if (!window.google || !window.google.accounts) return;
      window.google.accounts.id.initialize({ client_id: state.googleClientId, callback: window.handleGoogleCredentialResponse });
      window.google.accounts.id.renderButton(btnContainer, { theme: 'filled_black', size: 'large', width: 320, text: 'signin_with' });
    };

    if (window.google && window.google.accounts) init();
    else {
      const script = document.createElement('script');
      script.src = 'https://accounts.google.com/gsi/client';
      script.async = true;
      script.onload = init;
      document.head.appendChild(script);
    }
  }

  async function logout() {
    try { await api('/api/auth/logout', { method: 'POST' }); } catch (_) {}
    state.user = null;
    if (state.ws) { state.ws.close(); state.ws = null; }
    stopPresence();
    showAuthModal();
  }

  // ---------------------------------------------------------------- websocket
  function initWebSocket() {
    if (state.ws && (state.ws.readyState === WebSocket.OPEN || state.ws.readyState === WebSocket.CONNECTING)) return;
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${protocol}//${window.location.host}/ws`;
    const indicator = $('#ws-indicator');
    const statusText = indicator.querySelector('.status-text');
    try { state.ws = new WebSocket(wsUrl); } catch (_) { scheduleWsReconnect(); return; }

    state.ws.onopen = () => {
      indicator.classList.remove('disconnected');
      indicator.classList.add('connected');
      statusText.textContent = 'Live';
      state.wsAttempts = 0;
    };
    state.ws.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data);
        if (msg.type === 'reminder_triggered' && msg.data) {
          showReminderToast(msg.data);
          loadReminders();
        }
      } catch (_) {}
    };
    state.ws.onclose = () => {
      indicator.classList.remove('connected');
      indicator.classList.add('disconnected');
      statusText.textContent = 'Reconnecting…';
      scheduleWsReconnect();
    };
    state.ws.onerror = () => { try { state.ws.close(); } catch (_) {} };
  }

  function scheduleWsReconnect() {
    if (state.wsRetry) clearTimeout(state.wsRetry);
    const delay = Math.min(30000, 1000 * Math.pow(1.5, state.wsAttempts++));
    state.wsRetry = setTimeout(() => { if (state.user) initWebSocket(); }, delay);
  }

  // ---------------------------------------------------------------- folders
  async function loadFolders() {
    try {
      const data = await apiJson('/api/folders');
      state.folderTree = data.folders || [];
    } catch (err) {
      console.error('loadFolders failed:', err);
      return;
    }
    try { renderSidebarFolders(); } catch (err) { console.error('renderSidebarFolders failed:', err); }
    try { populateFolderSelects(); } catch (err) { console.error('populateFolderSelects failed:', err); }
  }

  function flattenFolders(tree, depth = 0, out = []) {
    for (const f of tree) {
      out.push({ id: f.id, name: f.name, icon: f.icon || '', depth, parentId: f.parent_id || null, noteCount: f.note_count || 0 });
      if (f.children) flattenFolders(f.children, depth + 1, out);
    }
    return out;
  }

  function folderPath(tree, targetId, trail = []) {
    for (const f of tree) {
      if (f.id === targetId) return [...trail, f.name];
      if (f.children) {
        const found = folderPath(f.children, targetId, [...trail, f.name]);
        if (found) return found;
      }
    }
    return null;
  }

  function renderSidebarFolders() {
    const box = $('#sidebar-folders');
    box.innerHTML = '';
    if (!state.folderTree.length) {
      box.innerHTML = '<div class="tags-empty">No folders yet.</div>';
      return;
    }
    const renderInto = (container, nodes, depth) => {
      for (const f of nodes) {
        const wrap = document.createElement('div');
        wrap.style.setProperty('--depth', depth);
        wrap.className = 'sidebar-folder-wrap';
        const row = document.createElement('div');
        row.className = `sidebar-folder${state.folderFilter && state.folderFilter.id === f.id ? ' active' : ''}`;
        row.style.setProperty('--depth', depth);
        row.innerHTML = `
          <div class="folder-row-main">
            <span class="folder-caret">${f.children && f.children.length ? '▾' : ''}</span>
            <button class="folder-btn" title="Open folder">
              <span class="folder-icon">${escapeHtml(f.icon || '📁')}</span>
              <span class="folder-name">${escapeHtml(f.name)}</span>
              <span class="tag-count">${f.note_count ?? 0}</span>
            </button>
            <div class="folder-row-actions">
              <button class="btn-icon btn-xs" data-act="sub" title="New subfolder">＋</button>
              <button class="btn-icon btn-xs" data-act="icon" title="Change symbol">🏷</button>
              <button class="btn-icon btn-xs" data-act="rename" title="Rename">✏️</button>
              <button class="btn-icon btn-xs" data-act="delete" title="Delete">🗑</button>
            </div>
          </div>
        `;
        row.querySelector('.folder-btn').onclick = () => filterByFolder(f.id);
        row.querySelector('[data-act="sub"]').onclick = () => openFolderModal(null, f.id);
        row.querySelector('[data-act="rename"]').onclick = () => openFolderModal(f, null);
        row.querySelector('[data-act="icon"]').onclick = () => pickIcon((icon) => updateFolder(f.id, { icon }), f.icon || '');
        row.querySelector('[data-act="delete"]').onclick = async () => {
          const ok = await confirmDialog(`Delete folder "${f.name}"? Subfolders move up, notes inside go back to the top level.`);
          if (!ok) return;
          try {
            await apiJson(`/api/folders/${f.id}`, { method: 'DELETE' });
            if (state.folderFilter && state.folderFilter.id === f.id) clearFolderFilter();
            await Promise.all([loadFolders(), loadNotes()]);
          } catch (err) { showToast('error', 'Delete failed', err.message); }
        };
        wrap.appendChild(row);
        if (f.children && f.children.length) renderInto(wrap, f.children, depth + 1);
        container.appendChild(wrap);
      }
    };
    renderInto(box, state.folderTree, 0);
  }

  function filterByFolder(id) {
    const flat = flattenFolders(state.folderTree);
    const target = flat.find((f) => f.id === id);
    if (!target) return;
    if (state.folderFilter && state.folderFilter.id === id) {
      clearFolderFilter();
      return;
    }
    const path = folderPath(state.folderTree, id) || [target.name];
    state.folderFilter = { id, name: target.name, path };
    if (state.view !== 'notes') switchView('notes');
    state.page = 1;
    renderActiveFilters();
    renderSidebarFolders();
    loadNotes();
  }

  function clearFolderFilter() {
    state.folderFilter = null;
    state.page = 1;
    renderActiveFilters();
    renderSidebarFolders();
    loadNotes();
  }

  function renderActiveFilters() {
    const box = $('#active-filters');
    box.innerHTML = '';
    const parts = [];
    if (state.folderFilter) {
      const chip = document.createElement('span');
      chip.className = 'tag-pill filter-chip';
      chip.innerHTML = `📁 ${escapeHtml(state.folderFilter.path.join(' / '))} <button class="tag-chip-x" data-clear="folder" title="Remove">×</button>`;
      parts.push(chip);
    }
    for (const t of state.tagFilters) {
      const chip = document.createElement('span');
      chip.className = 'tag-pill filter-chip';
      chip.innerHTML = `#${escapeHtml(t)} <button class="tag-chip-x" data-clear="tag" data-tag="${escapeHtml(t)}" title="Remove">×</button>`;
      parts.push(chip);
    }
    if (!parts.length) { box.classList.add('hidden'); return; }
    for (const p of parts) box.appendChild(p);
    box.classList.remove('hidden');
    box.querySelectorAll('[data-clear="folder"]').forEach((b) => (b.onclick = clearFolderFilter));
    box.querySelectorAll('[data-clear="tag"]').forEach((b) => {
      b.onclick = () => {
        state.tagFilters = state.tagFilters.filter((t) => t !== b.dataset.tag);
        state.page = 1;
        renderActiveFilters();
        renderSidebarTags();
        loadNotes();
      };
    });
  }

  // Folder create/edit modal
  function openFolderModal(folder, parentId) {
    const fe = state.folderEditor;
    fe.folder = folder;
    fe.icon = folder ? folder.icon || '' : '';
    $('#folder-modal-title').textContent = folder ? 'Edit Folder' : 'New Folder';
    $('#folder-name-input').value = folder ? folder.name : '';
    populateFolderSelects($('#folder-parent-select'), folder ? folder.id : null, parentId || '');
    $('#btn-folder-icon').innerHTML = `${fe.icon || '🏷'} ${fe.icon ? '' : 'None'}`;
    $('#modal-folder').classList.remove('hidden');
    setTimeout(() => $('#folder-name-input').focus(), 30);
  }

  function closeFolderModal() { $('#modal-folder').classList.add('hidden'); state.folderEditor.folder = null; }

  function populateFolderSelects(selectEl, excludeId, selectedId) {
    if (!selectEl) return;
    const flat = flattenFolders(state.folderTree);
    const opts = flat
      .filter((f) => !excludeId || f.id !== excludeId)
      .map((f) => `<option value="${escapeHtml(f.id)}"${f.id === selectedId ? ' selected' : ''}>${'—'.repeat(f.depth)} ${escapeHtml(f.icon || '📁')} ${escapeHtml(f.name)}</option>`)
      .join('');
    const topLevelSelected = selectedId === '' || selectedId === undefined;
    selectEl.innerHTML = `<option value=""${topLevelSelected ? ' selected' : ''}>Top level (no folder)</option>${opts}`;
  }

  async function saveFolder() {
    const fe = state.folderEditor;
    const name = $('#folder-name-input').value.trim();
    if (!name) { showToast('error', 'Missing name', 'Give the folder a name.'); return; }
    const parentId = $('#folder-parent-select').value || null;
    try {
      if (fe.folder) {
        await apiJson(`/api/folders/${fe.folder.id}`, { method: 'PUT', body: { name, parent_id: parentId, icon: fe.icon || null } });
      } else {
        await apiJson('/api/folders', { method: 'POST', body: { name, parent_id: parentId, icon: fe.icon || null } });
      }
      closeFolderModal();
      await loadFolders();
      showToast('success', 'Folder saved', name);
    } catch (err) {
      showToast('error', 'Save failed', err.message);
    }
  }

  async function updateFolder(id, payload) {
    try {
      await apiJson(`/api/folders/${id}`, { method: 'PUT', body: payload });
      await loadFolders();
    } catch (err) { showToast('error', 'Update failed', err.message); }
  }

  // ---------------------------------------------------------------- icon picker
  const ICONS = [
    '📝','📄','📓','📔','📒','📕','📗','📘','📙','📚','🗒️','🗂️','📁','📂','💼','🎒','🧰','🧠','💡','⚡','🔥','✨','⭐','🌟','🌈','☀️','🌙','⛅','🌧️','❄️','🌊',
    '🚀','✈️','🚗','🚲','🚂','⛵','🏠','🏢','🏗️','🗺️','📍','📌','🔖','🏷️','🎯','🎨','🎵','🎬','🎮','🎲','🏆','🏅','🥇','💰','💳','📈','📉','📊','💹','🧮',
    '🍎','🍕','☕','🍺','🥗','🍳','🥕','🛒','💊','⚕️','🩺','🔬','🧪','⚙️','🔧','🔨','💻','🖥️','📱','🖨️','⌨️','🖱️','📷','🎥','📞','🔋','🔌','🌐','🔒','🔑',
    '❤️','💚','💙','💜','🧡','💛','🤍','💯','✅','❌','⚠️','❗','❓','💬','📢','🔔','📣','⏰','⏳','📅','🗓️','📆','🕐','🧭','🎁','🎉','👋','👍','👀','🤖',
    '1️⃣','2️⃣','3️⃣','4️⃣','5️⃣','🔺','🔻','➡️','↗️','🔁','♻️','⚖️','📜','⚖️','🗝️','🔗','🧲','🧩','🎭','🎪','🚨','🚦','🛑','🏁','☑️','🆕','🆒','🆓','💤','☠️',
  ];

  function openIconPicker(anchorEl, onPick, current) {
    const pop = $('#icon-picker-popover');
    const grid = $('#icon-picker-grid');
    grid.innerHTML = '';
    for (const icon of ICONS) {
      const btn = document.createElement('button');
      btn.className = 'icon-picker-item';
      btn.textContent = icon;
      btn.onclick = () => { onPick(icon); pop.classList.add('hidden'); };
      grid.appendChild(btn);
    }
    $('#btn-icon-none').onclick = () => { onPick(''); pop.classList.add('hidden'); };
    const rect = anchorEl.getBoundingClientRect();
    pop.style.top = `${Math.min(rect.bottom + 6, window.innerHeight - 340)}px`;
    pop.style.left = `${Math.max(8, Math.min(rect.left, window.innerWidth - 320))}px`;
    pop.classList.remove('hidden');
  }

  function pickIcon(onPick, current) {
    // Uses the note icon button as anchor
    openIconPicker($('#btn-note-icon') || document.body, onPick, current);
  }

  // ---------------------------------------------------------------- tags
  async function loadTags() {
    try {
      const data = await apiJson('/api/tags');
      state.tags = data.tags || [];
      renderSidebarTags();
    } catch (err) { console.error('loadTags failed:', err); }
  }

  function renderSidebarTags() {
    const box = $('#sidebar-tags');
    box.innerHTML = '';
    if (!state.tags.length) {
      box.innerHTML = '<div class="tags-empty">No tags yet — add some on a note.</div>';
      return;
    }
    for (const tag of state.tags) {
      const selected = state.tagFilters.includes(tag.name);
      const item = document.createElement('div');
      item.className = `sidebar-tag${selected ? ' active' : ''}`;
      item.innerHTML = `
        <button class="sidebar-tag-main" title="${selected ? 'Remove filter' : 'Filter by'} #${escapeHtml(tag.name)}">
          <span class="tag-pill tag-pill-sm"><span class="tag-dot"${tag.color ? ` style="background:${escapeHtml(tag.color)}"` : ''}></span>#${escapeHtml(tag.name)}</span>
          <span class="tag-count">${tag.note_count ?? 0}</span>
        </button>
        <div class="sidebar-tag-actions">
          <button class="btn-icon btn-xs" data-act="rename" title="Rename">✏️</button>
          <button class="btn-icon btn-xs" data-act="delete" title="Delete tag">🗑</button>
        </div>
        <input class="tag-rename-input hidden" value="${escapeHtml(tag.name)}" data-id="${tag.id}">
      `;
      item.querySelector('.sidebar-tag-main').onclick = () => toggleTagFilter(tag.name);
      item.querySelector('[data-act="rename"]').onclick = () => startTagRename(item, tag);
      item.querySelector('[data-act="delete"]').onclick = async () => {
        const ok = await confirmDialog(`Delete tag "#${tag.name}"? It will be removed from all notes.`);
        if (!ok) return;
        try {
          await apiJson(`/api/tags/${tag.id}`, { method: 'DELETE' });
          state.tagFilters = state.tagFilters.filter((t) => t !== tag.name);
          await Promise.all([loadTags(), loadNotes()]);
          renderActiveFilters();
        } catch (err) { showToast('error', 'Delete failed', err.message); }
      };
      box.appendChild(item);
    }
  }

  function toggleTagFilter(name) {
    if (state.tagFilters.includes(name)) state.tagFilters = state.tagFilters.filter((t) => t !== name);
    else state.tagFilters.push(name);
    if (state.view !== 'notes') switchView('notes');
    state.page = 1;
    renderActiveFilters();
    renderSidebarTags();
    loadNotes();
  }

  function startTagRename(item, tag) {
    const pill = item.querySelector('.sidebar-tag-main');
    const input = item.querySelector('.tag-rename-input');
    pill.classList.add('hidden');
    item.querySelector('.sidebar-tag-actions').classList.add('hidden');
    input.classList.remove('hidden');
    input.focus();
    input.select();
    const commit = async () => {
      const newName = input.value.trim();
      if (!newName || newName === tag.name) { renderSidebarTags(); return; }
      try {
        await apiJson(`/api/tags/${tag.id}`, { method: 'PUT', body: { name: newName } });
        state.tagFilters = state.tagFilters.map((t) => (t === tag.name ? newName : t));
        await Promise.all([loadTags(), loadNotes()]);
        renderActiveFilters();
      } catch (err) {
        showToast('error', 'Rename failed', err.message);
        renderSidebarTags();
      }
    };
    input.onblur = commit;
    input.onkeydown = (e) => {
      if (e.key === 'Enter') { e.preventDefault(); commit(); }
      if (e.key === 'Escape') renderSidebarTags();
    };
  }

  // ---------------------------------------------------------------- notes
  function notesQueryUrl() {
    const params = new URLSearchParams();
    params.set('page', String(state.page));
    params.set('limit', String(state.pageSize));
    if (state.view === 'archive') params.set('archived', 'true');
    if (state.view === 'notes' && state.noteFilter !== 'all') {
      if (['simple', 'detailed', 'code', 'sheet'].includes(state.noteFilter)) params.set('type', state.noteFilter);
      else if (state.noteFilter === 'pinned') params.set('pinned', 'true');
    }
    if (state.searchQuery) {
      params.set('search', state.searchQuery);
      const scopes = Object.entries(state.searchIn).filter(([, on]) => on).map(([k]) => k);
      if (scopes.length && scopes.length < 4) params.set('search_in', scopes.join(','));
    }
    if (state.tagFilters.length) params.set('tag', state.tagFilters.join(','));
    if (state.folderFilter) params.set('folder', state.folderFilter.id);
    return `/api/notes?${params.toString()}`;
  }

  async function loadNotes() {
    try {
      const data = await apiJson(notesQueryUrl());
      state.notes = data.notes || [];
      state.totalNotes = data.pagination ? data.pagination.total : 0;
      renderNotes();
    } catch (err) {
      console.error('loadNotes failed:', err);
    }
  }

  function renderNotes() {
    const grid = state.view === 'archive' ? $('#archive-grid') : $('#notes-grid');
    const empty = state.view === 'archive' ? $('#archive-empty-state') : $('#notes-empty-state');
    const pagination = $('#notes-pagination');

    grid.innerHTML = '';
    const filtering = !!state.searchQuery || state.tagFilters.length > 0 || !!state.folderFilter;

    if (!state.notes.length) {
      empty.classList.remove('hidden');
      if (state.view !== 'archive') {
        $('#notes-empty-title').textContent = filtering ? 'No results' : 'No notes yet';
        $('#notes-empty-subtitle').textContent = filtering
          ? 'Try different keywords, or clear the search / tag / folder filters.'
          : 'Capture a thought in the bar above, or use the sidebar buttons.';
      }
      pagination.classList.add('hidden');
      return;
    }

    empty.classList.add('hidden');
    for (const note of state.notes) grid.appendChild(createNoteCard(note));

    const totalPages = Math.max(1, Math.ceil(state.totalNotes / state.pageSize));
    if (totalPages > 1) {
      pagination.classList.remove('hidden');
      $('#pagination-info').textContent = `Page ${state.page} of ${totalPages}`;
      $('#btn-prev-page').disabled = state.page <= 1;
      $('#btn-next-page').disabled = state.page >= totalPages;
    } else {
      pagination.classList.add('hidden');
    }
    $('#badge-total-notes').textContent = state.totalNotes;
  }

  const TYPE_META = {
    simple: { label: 'TXT', cls: 'badge-simple' },
    detailed: { label: 'MD', cls: 'badge-detailed' },
    code: { label: 'CODE', cls: 'badge-code' },
    sheet: { label: 'SHEET', cls: 'badge-sheet' },
  };

  function createNoteCard(note) {
    const card = document.createElement('article');
    card.className = `note-card${note.is_pinned ? ' pinned' : ''}`;
    if (note.color) card.style.setProperty('--accent', note.color);

    const meta = TYPE_META[note.note_type] || TYPE_META.simple;
    const q = state.searchQuery;
    const tagsHtml = (note.tags || []).map((t) => `<span class="tag-pill tag-pill-sm">#${escapeHtml(t)}</span>`).join('');
    const folderHtml = note.folder_name ? `<span class="folder-chip">${escapeHtml(note.folder_name)}</span>` : '';
    const plainSnippet = noteSnippet(note, 180);
    const iconHtml = note.icon ? `<span class="note-card-icon">${escapeHtml(note.icon)}</span>` : '';

    card.innerHTML = `
      <div class="note-card-top">
        <span class="type-badge ${meta.cls}">${meta.label}</span>
        <div class="card-actions">
          ${note.reminder_count ? `<span class="card-reminder-count" title="Linked reminders">⏰${note.reminder_count}</span>` : ''}
          <button class="btn-icon btn-card-pin" title="${note.is_pinned ? 'Unpin' : 'Pin'}">${note.is_pinned ? '📌' : '📍'}</button>
          <button class="btn-icon btn-card-archive" title="${note.is_archived ? 'Unarchive' : 'Archive'}">${note.is_archived ? '📥' : '📦'}</button>
        </div>
      </div>
      <h4 class="note-card-title">${iconHtml}${highlight(note.title || 'Untitled', q)}</h4>
      <div class="note-card-snippet${note.note_type === 'code' ? ' code-snippet' : ''}">${plainSnippet ? highlight(plainSnippet, q) : '<em>Empty note</em>'}</div>
      ${tagsHtml || folderHtml ? `<div class="note-card-tags">${folderHtml}${tagsHtml}</div>` : ''}
      <div class="note-card-footer">
        <span>${formatDate(note.updated_at || note.created_at)}</span>
      </div>
    `;

    card.onclick = () => openNoteEditor(note);
    card.querySelector('.btn-card-pin').onclick = async (e) => { e.stopPropagation(); await togglePin(note); loadNotes(); };
    card.querySelector('.btn-card-archive').onclick = async (e) => { e.stopPropagation(); await toggleArchive(note); loadNotes(); };
    return card;
  }

  async function togglePin(note) {
    try { await apiJson(`/api/notes/${note.id}/pin`, { method: 'PUT' }); } catch (err) { showToast('error', 'Pin failed', err.message); }
  }

  async function toggleArchive(note) {
    try { await apiJson(`/api/notes/${note.id}/archive`, { method: 'PUT' }); } catch (err) { showToast('error', 'Archive failed', err.message); }
  }

  // ---------------------------------------------------------------- note editor
  function openNoteEditor(note, newType) {
    const ed = state.editor;
    ed.note = note;
    ed.type = note ? note.note_type : (newType || 'simple');
    ed.color = note ? note.color || '' : '';
    ed.icon = note ? note.icon || '' : '';
    ed.tags = note ? [...(note.tags || [])] : [];
    ed.folderId = note ? note.folder_id || '' : '';
    ed.pinned = note ? !!note.is_pinned : false;
    ed.archived = note ? !!note.is_archived : false;
    ed.lang = 'text';
    ed.sheet = null;
    ed.dirty = false;

    $('#note-title-input').value = note ? note.title : '';
    $('#note-content-input').value = note ? note.content : '';
    $('#btn-delete-note').classList.toggle('hidden', !note);
    $('#btn-versions').classList.toggle('hidden', !note);
    $('#btn-share-note').classList.toggle('hidden', !note);
    $('#btn-export-menu').classList.toggle('hidden', !note);
    $('#export-dropdown').classList.add('hidden');
    $('#btn-note-icon').textContent = ed.icon || '🏷';
    populateFolderSelects($('#note-folder-select'), null, ed.folderId);

    applyEditorType();
    renderTagChips();
    renderSuggestedTags();
    updateEditorButtons();
    renderAiEditorGroup();

    $('#modal-note').classList.remove('hidden');
    setTimeout(() => $('#note-title-input').focus(), 30);

    // Presence: tell the server this device has the note open
    if (note) {
      startPresence(note.id);
      refreshPresenceBanner(note.id);
    } else {
      stopPresence();
      $('#presence-banner').classList.add('hidden');
    }
  }

  function closeNoteEditor() {
    $('#modal-note').classList.add('hidden');
    hideTagSuggestDropdown();
    $('#icon-picker-popover').classList.add('hidden');
    stopPresence();
    state.editor.note = null;
    setAutosaveIndicator('');
  }

  function applyEditorType() {
    const ed = state.editor;
    $$('#editor-type-seg button').forEach((b) => b.classList.toggle('active', b.dataset.noteType === ed.type));

    const toolbar = $('#markdown-toolbar');
    const preview = $('#preview-pane');
    const panes = $('#editor-panes');
    const sheetToolbar = $('#sheet-toolbar');
    const sheetEditor = $('#sheet-editor');
    const codeLangWrap = $('#code-lang-wrap');
    const contentInput = $('#note-content-input');

    toolbar.classList.add('hidden');
    sheetToolbar.classList.add('hidden');
    sheetEditor.classList.add('hidden');
    sheetEditor.innerHTML = '';
    codeLangWrap.classList.add('hidden');
    preview.classList.add('hidden');
    contentInput.classList.remove('hidden');
    panes.classList.remove('hidden');

    if (ed.type === 'detailed') {
      toolbar.classList.remove('hidden');
      preview.classList.remove('hidden');
      preview.innerHTML = renderMarkdownClient(contentInput.value);
      renderMermaidBlocks(preview);
    } else if (ed.type === 'code') {
      codeLangWrap.classList.remove('hidden');
      $('#code-lang-select').value = ed.lang || 'text';
      preview.classList.remove('hidden');
      refreshCodePreview();
    } else if (ed.type === 'sheet') {
      sheetToolbar.classList.remove('hidden');
      sheetEditor.classList.remove('hidden');
      panes.classList.add('hidden');
      contentInput.classList.add('hidden');
      loadSheetGrid();
    }
  }

  function updateEditorButtons() {
    const ed = state.editor;
    $('#btn-pin-note').textContent = ed.pinned ? '📌 Unpin' : '📍 Pin';
    $('#btn-archive-note').textContent = ed.archived ? 'Unarchive' : 'Archive';
    $$('#color-picker-group .color-dot').forEach((dot) => dot.classList.toggle('active', dot.dataset.color === ed.color));
  }

  function setAutosaveIndicator(text) {
    $('#autosave-indicator').textContent = text;
  }

  async function refreshCodePreview() {
    const ed = state.editor;
    const code = $('#note-content-input').value;
    const lang = $('#code-lang-select').value || 'text';
    ed.lang = lang;
    if (!code.trim()) { $('#preview-pane').innerHTML = ''; return; }
    try {
      const data = await apiJson('/api/highlight', { method: 'POST', body: { language: lang, code } });
      $('#preview-pane').innerHTML = data.html || '';
    } catch (_) {
      $('#preview-pane').innerHTML = `<pre><code>${escapeHtml(code)}</code></pre>`;
    }
  }

  // Sheet editor
  function loadSheetGrid() {
    const ed = state.editor;
    let rows;
    try {
      rows = JSON.parse($('#note-content-input').value || '[]');
      if (!Array.isArray(rows) || !rows.length) rows = [['', ''], ['', '']];
    } catch (_) {
      rows = [['', ''], ['', '']];
    }
    // Normalize into a rectangular grid
    const cols = Math.max(2, ...rows.map((r) => (Array.isArray(r) ? r.length : 0)));
    ed.sheet = rows.map((r) => Array.from({ length: cols }, (_, i) => String(Array.isArray(r) ? r[i] ?? '' : r ?? '')));
    renderSheetGrid();
  }

  function renderSheetGrid() {
    const ed = state.editor;
    const box = $('#sheet-editor');
    box.innerHTML = '';
    if (!ed.sheet) return;
    const table = document.createElement('table');
    table.className = 'sheet-grid';
    ed.sheet.forEach((row, ri) => {
      const tr = document.createElement('tr');
      row.forEach((cell, ci) => {
        const td = document.createElement('td');
        const input = document.createElement('input');
        input.value = cell;
        input.dataset.ri = ri;
        input.dataset.ci = ci;
        input.oninput = () => { ed.sheet[ri][ci] = input.value; markEditorDirty(); };
        td.appendChild(input);
        tr.appendChild(td);
      });
      table.appendChild(tr);
    });
    box.appendChild(table);
  }

  function sheetRowsToContent() {
    const ed = state.editor;
    if (!ed.sheet) return '';
    return JSON.stringify(ed.sheet.map((r) => r.map((c) => String(c ?? ''))));
  }

  function sheetAddRow() {
    const ed = state.editor;
    if (!ed.sheet) ed.sheet = [['', ''], ['', '']];
    ed.sheet.push(ed.sheet[0].map(() => ''));
    renderSheetGrid();
    markEditorDirty();
  }

  function sheetAddCol() {
    const ed = state.editor;
    if (!ed.sheet) ed.sheet = [['', ''], ['', '']];
    ed.sheet.forEach((r) => r.push(''));
    renderSheetGrid();
    markEditorDirty();
  }

  function sheetDelRow() {
    const ed = state.editor;
    if (!ed.sheet || ed.sheet.length <= 1) return;
    ed.sheet.pop();
    renderSheetGrid();
    markEditorDirty();
  }

  function sheetDelCol() {
    const ed = state.editor;
    if (!ed.sheet || ed.sheet[0].length <= 1) return;
    ed.sheet.forEach((r) => r.pop());
    renderSheetGrid();
    markEditorDirty();
  }

  function markEditorDirty() {
    state.editor.dirty = true;
    if (state.editor.note) scheduleAutosave();
  }

  // ---------------------------------------------------------------- autosave
  let autosaveTimer = null;
  function scheduleAutosave() {
    setAutosaveIndicator('Unsaved…');
    if (autosaveTimer) clearTimeout(autosaveTimer);
    autosaveTimer = setTimeout(() => saveNote(true), 1200);
  }

  async function saveNote(silent) {
    const ed = state.editor;
    if (ed.saving) return;
    const title = $('#note-title-input').value.trim();
    const content = ed.type === 'sheet' ? sheetRowsToContent() : $('#note-content-input').value;
    if (!title && !content.trim() && !ed.note) return;

    ed.saving = true;
    if (!silent) setAutosaveIndicator('Saving…');

    const payload = {
      title: title || 'Untitled Note',
      content,
      note_type: ed.type,
      color: ed.color || null,
      icon: ed.icon || null,
      tags: ed.tags,
      is_pinned: ed.pinned,
      is_archived: ed.archived,
    };
    if (ed.note) {
      payload.folder_id = ed.folderId || null;
    } else {
      payload.folder_id = ed.folderId || null;
    }

    try {
      if (ed.note) {
        await apiJson(`/api/notes/${ed.note.id}`, { method: 'PUT', body: payload });
      } else {
        const created = await apiJson('/api/notes', { method: 'POST', body: payload });
        ed.note = created;
        $('#btn-delete-note').classList.remove('hidden');
        $('#btn-versions').classList.remove('hidden');
        $('#btn-share-note').classList.remove('hidden');
        $('#btn-export-menu').classList.remove('hidden');
        startPresence(created.id);
        refreshPresenceBanner(created.id);
        await Promise.all([loadNotes(), loadTags(), loadFolders()]);
      }
      ed.dirty = false;
      if (silent) {
        setAutosaveIndicator(`Saved ${new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`);
      } else {
        setAutosaveIndicator('');
        closeNoteEditor();
        await Promise.all([loadNotes(), loadTags(), loadFolders()]);
        showToast('success', 'Saved', payload.title);
      }
    } catch (err) {
      setAutosaveIndicator('Save failed — click Save to retry');
      if (!silent) showToast('error', 'Save failed', err.message);
    } finally {
      ed.saving = false;
    }
  }

  async function deleteActiveNote() {
    const ed = state.editor;
    if (!ed.note) return;
    const ok = await confirmDialog('Delete this note? It will be removed.');
    if (!ok) return;
    try {
      await apiJson(`/api/notes/${ed.note.id}`, { method: 'DELETE' });
      stopPresence();
      closeNoteEditor();
      await Promise.all([loadNotes(), loadTags(), loadFolders()]);
      showToast('success', 'Deleted', 'Note deleted.');
    } catch (err) { showToast('error', 'Delete failed', err.message); }
  }

  async function pinActiveNote() {
    const ed = state.editor;
    ed.pinned = !ed.pinned;
    updateEditorButtons();
    if (ed.note) {
      try { await apiJson(`/api/notes/${ed.note.id}/pin`, { method: 'PUT' }); loadNotes(); }
      catch (err) { showToast('error', 'Pin failed', err.message); }
    }
  }

  async function archiveActiveNote() {
    const ed = state.editor;
    ed.archived = !ed.archived;
    updateEditorButtons();
    if (ed.note) {
      try {
        await apiJson(`/api/notes/${ed.note.id}/archive`, { method: 'PUT' });
        closeNoteEditor();
        await Promise.all([loadNotes(), loadTags(), loadFolders()]);
      } catch (err) { showToast('error', 'Archive failed', err.message); }
    }
  }

  function exportActiveNote(format) {
    const ed = state.editor;
    if (!ed.note) return;
    window.location.href = `/api/notes/${ed.note.id}/export${format === 'pdf' ? '?format=pdf' : ''}`;
  }

  // ---------------------------------------------------------------- presence
  function startPresence(noteId) {
    stopPresence();
    state.editor.presenceTimer = setInterval(async () => {
      try {
        await apiJson(`/api/notes/${noteId}/presence`, { method: 'POST', body: { device: state.deviceId } });
      } catch (_) {}
    }, 30000);
    // Immediate heartbeat
    apiJson(`/api/notes/${noteId}/presence`, { method: 'POST', body: { device: state.deviceId } }).catch(() => {});
  }

  function stopPresence() {
    if (state.editor.presenceTimer) { clearInterval(state.editor.presenceTimer); state.editor.presenceTimer = null; }
    const noteId = state.editor.note ? state.editor.note.id : null;
    if (noteId) {
      api(`/api/notes/${noteId}/presence`, { method: 'DELETE', body: { device: state.deviceId } }).catch(() => {});
    }
  }

  async function refreshPresenceBanner(noteId) {
    const banner = $('#presence-banner');
    try {
      const data = await apiJson(`/api/notes/${noteId}?device=${encodeURIComponent(state.deviceId)}`);
      const editors = data.active_editors || [];
      if (editors.length) {
        banner.innerHTML = `⚠️ This note is also open on: <strong>${escapeHtml(editors.map((e) => e.device).join(', '))}</strong>. Concurrent edits may conflict.`;
        banner.classList.remove('hidden');
      } else {
        banner.classList.add('hidden');
      }
    } catch (_) {
      banner.classList.add('hidden');
    }
  }

  // ---------------------------------------------------------------- versions
  async function openVersions() {
    const ed = state.editor;
    if (!ed.note) return;
    const list = $('#versions-list');
    list.innerHTML = '<div class="versions-loading">Loading…</div>';
    $('#modal-versions').classList.remove('hidden');
    try {
      const data = await apiJson(`/api/notes/${ed.note.id}/versions`);
      const versions = data.versions || [];
      list.innerHTML = '';
      if (!versions.length) {
        list.innerHTML = '<div class="versions-loading">No versions yet — they appear automatically as you edit (max 10, 15 min apart).</div>';
        return;
      }
      for (const v of versions) {
        const row = document.createElement('div');
        row.className = 'version-row';
        row.innerHTML = `
          <div class="version-info">
            <span class="version-time">${formatDate(v.created_at)}</span>
            <span class="version-title">${escapeHtml(v.title)}</span>
            <span class="version-snippet">${escapeHtml(noteSnippet({ note_type: v.note_type, content: v.content }, 90))}</span>
          </div>
          <button class="btn btn-glass btn-sm" data-act="restore">Restore</button>
        `;
        row.querySelector('[data-act="restore"]').onclick = async () => {
          const ok = await confirmDialog('Restore this version? The current content will be replaced (the current state is kept as a new version first).');
          if (!ok) return;
          try {
            await apiJson(`/api/notes/${ed.note.id}/restore`, { method: 'POST', body: { version_id: v.id } });
            const fresh = await apiJson(`/api/notes/${ed.note.id}?device=${encodeURIComponent(state.deviceId)}`);
            $('#note-title-input').value = fresh.note.title;
            $('#note-content-input').value = fresh.note.content;
            ed.type = fresh.note.note_type;
            ed.tags = fresh.note.tags || [];
            applyEditorType();
            renderTagChips();
            $('#modal-versions').classList.add('hidden');
            showToast('success', 'Restored', 'Version restored.');
          } catch (err) { showToast('error', 'Restore failed', err.message); }
        };
        list.appendChild(row);
      }
    } catch (err) {
      list.innerHTML = `<div class="versions-loading">${escapeHtml(err.message)}</div>`;
    }
  }

  async function clearVersions() {
    const ed = state.editor;
    if (!ed.note) return;
    const ok = await confirmDialog('Clear ALL old versions of this note? This cannot be undone.');
    if (!ok) return;
    try {
      await apiJson(`/api/notes/${ed.note.id}/versions/clear`, { method: 'POST' });
      $('#modal-versions').classList.add('hidden');
      showToast('success', 'Cleared', 'Version history cleared.');
    } catch (err) { showToast('error', 'Clear failed', err.message); }
  }

  // ---------------------------------------------------------------- sharing
  async function openShare() {
    const ed = state.editor;
    if (!ed.note) return;
    state.shareState = { noteId: ed.note.id, share: null };
    $('#share-link-row').classList.add('hidden');
    $('#modal-share').classList.remove('hidden');
    try {
      const data = await apiJson(`/api/notes/${ed.note.id}/share`);
      if (data.share) showShareLink(data.share);
    } catch (_) {}
  }

  function showShareLink(share) {
    state.shareState.share = share;
    const url = `${window.location.origin}${share.path}`;
    $('#share-link-input').value = url;
    $('#share-link-row').classList.remove('hidden');
    $('#share-link-row').querySelector('label').textContent =
      share.mode === 'snapshot' ? 'Public link (frozen copy 📸)' : 'Public link (always updated 🔄)';
  }

  async function createShare(mode) {
    const ed = state.editor;
    if (!ed.note) return;
    try {
      const data = await apiJson(`/api/notes/${ed.note.id}/share`, { method: 'POST', body: { mode } });
      showShareLink(data.share);
    } catch (err) { showToast('error', 'Share failed', err.message); }
  }

  async function revokeShare() {
    const ed = state.editor;
    if (!ed.note) return;
    try {
      await apiJson(`/api/notes/${ed.note.id}/share`, { method: 'DELETE' });
      state.shareState.share = null;
      $('#share-link-row').classList.add('hidden');
      showToast('success', 'Revoked', 'Public link disabled.');
    } catch (err) { showToast('error', 'Revoke failed', err.message); }
  }

  // ---------------------------------------------------------------- tag editor
  function renderTagChips() {
    const box = $('#note-tags-chips');
    box.innerHTML = '';
    state.editor.tags.forEach((name, idx) => {
      const chip = document.createElement('span');
      chip.className = 'tag-pill tag-pill-editable';
      chip.innerHTML = `#${escapeHtml(name)}<button class="tag-chip-x" data-idx="${idx}" title="Remove">×</button>`;
      chip.querySelector('.tag-chip-x').onclick = () => {
        state.editor.tags.splice(idx, 1);
        renderTagChips();
        markEditorDirty();
      };
      box.appendChild(chip);
    });
  }

  function addEditorTag(name) {
    const clean = name.trim().replace(/^#/, '').replace(/\s+/g, ' ');
    if (!clean) return;
    if (!state.editor.tags.some((t) => t.toLowerCase() === clean.toLowerCase())) {
      state.editor.tags.push(clean);
      renderTagChips();
      markEditorDirty();
    }
    $('#note-tags-input').value = '';
    hideTagSuggestDropdown();
  }

  function hideTagSuggestDropdown() {
    $('#tag-suggest-dropdown').classList.add('hidden');
    $('#tag-suggest-dropdown').innerHTML = '';
  }

  function showTagSuggestions(list, query) {
    const dd = $('#tag-suggest-dropdown');
    dd.innerHTML = '';
    if (!list.length) { dd.classList.add('hidden'); return; }
    for (const tag of list) {
      const row = document.createElement('button');
      row.className = 'tag-suggest-item';
      row.innerHTML = `<span class="tag-pill tag-pill-sm">#${highlight(tag.name, query)}</span><span class="tag-count">${tag.note_count ?? 0}</span>`;
      row.onmousedown = (e) => { e.preventDefault(); addEditorTag(tag.name); };
      dd.appendChild(row);
    }
    dd.classList.remove('hidden');
  }

  function renderSuggestedTags() {
    const row = $('#tag-suggested-row');
    const content = $('#note-content-input').value || '';
    const found = new Set();
    const re = /(?:^|\s)#([\p{L}\p{N}_-]{1,40})/gu;
    let m;
    while ((m = re.exec(content)) && found.size < 8) found.add(m[1]);
    row.innerHTML = '';
    if (!found.size) return;
    const label = document.createElement('span');
    label.className = 'suggested-label';
    label.textContent = 'From note:';
    row.appendChild(label);
    for (const name of found) {
      if (state.editor.tags.some((t) => t.toLowerCase() === name.toLowerCase())) continue;
      const chip = document.createElement('button');
      chip.className = 'tag-pill tag-pill-suggested';
      chip.innerHTML = `+ #${escapeHtml(name)}`;
      chip.title = 'Add as tag';
      chip.onclick = () => { addEditorTag(name); renderSuggestedTags(); };
      row.appendChild(chip);
    }
  }

  // ---------------------------------------------------------------- AI
  function renderAiHeader() {
    const toggle = $('#btn-ai-toggle');
    const group = $('#ai-header-group');
    if (!state.capabilities.ai) {
      toggle.classList.add('hidden');
      group.classList.add('hidden');
      return;
    }
    toggle.classList.remove('hidden');
    updateAiToggleButton();
    const select = $('#ai-model-select');
    select.innerHTML = state.capabilities.aiModels
      .map((m) => `<option value="${escapeHtml(m)}"${m === state.ai.model ? ' selected' : ''}>${escapeHtml(m)}</option>`)
      .join('');
    if (state.ai.enabled) group.classList.remove('hidden');
    else group.classList.add('hidden');
  }

  function updateAiToggleButton() {
    $('#btn-ai-toggle').textContent = state.ai.enabled ? '✨ AI: On' : '✨ AI: Off';
    $('#btn-ai-toggle').classList.toggle('btn-primary', state.ai.enabled);
    $('#btn-ai-toggle').classList.toggle('btn-glass', !state.ai.enabled);
  }

  function renderAiEditorGroup() {
    const group = $('#ai-editor-group');
    if (!state.capabilities.ai || !state.ai.enabled) {
      group.classList.add('hidden');
      return;
    }
    group.classList.remove('hidden');
    const select = $('#ai-style-select');
    select.innerHTML = state.capabilities.aiStyles
      .map((s) => `<option value="${escapeHtml(s.id)}"${s.id === state.ai.style ? ' selected' : ''}>${escapeHtml(s.label)}</option>`)
      .join('');
  }

  async function aiRewrite() {
    const ed = state.editor;
    const content = ed.type === 'sheet' ? sheetRowsToContent() : $('#note-content-input').value;
    if (!content.trim()) { showToast('error', 'Nothing to rewrite', 'Write something first.'); return; }
    const btn = $('#btn-ai-rewrite');
    btn.disabled = true;
    btn.textContent = '✨ Rewriting…';
    try {
      const data = await apiJson('/api/ai/rewrite', {
        method: 'POST',
        body: { text: content, model: state.ai.model, style: state.ai.style },
      });
      $('#note-content-input').value = data.rewritten;
      if (ed.type === 'detailed') {
        $('#preview-pane').innerHTML = renderMarkdownClient(data.rewritten);
        renderMermaidBlocks($('#preview-pane'));
      }
      renderSuggestedTags();
      markEditorDirty();
      showToast('success', 'Rewritten', 'AI produced a new version — review and save.');
    } catch (err) {
      showToast('error', 'AI rewrite failed', err.message);
    } finally {
      btn.disabled = false;
      btn.textContent = '✨ Rewrite';
    }
  }

  // ---------------------------------------------------------------- reminders
  async function loadReminders() {
    try {
      const data = await apiJson('/api/reminders?limit=200');
      state.reminders = data.reminders || [];
      renderReminders();
    } catch (err) { console.error('loadReminders failed:', err); }
  }

  function renderReminders() {
    const list = $('#reminders-list');
    const empty = $('#reminders-empty-state');
    list.innerHTML = '';

    const filtered = state.reminders.filter((r) => {
      const s = r.state || 'pending';
      return state.reminderFilter === 'all' || s === state.reminderFilter;
    });

    const pendingCount = state.reminders.filter((r) => r.state === 'pending' || r.state === 'snoozed').length;
    const badge = $('#badge-pending-reminders');
    badge.textContent = pendingCount;
    badge.classList.toggle('badge-alert', pendingCount > 0);

    if (!filtered.length) { empty.classList.remove('hidden'); return; }
    empty.classList.add('hidden');
    for (const rem of filtered) list.appendChild(createReminderRow(rem));
  }

  const CHANNEL_META = {
    web: { icon: '🌐', label: 'Web' },
    email: { icon: '✉️', label: 'Email' },
    telegram: { icon: '✈️', label: 'Telegram' },
  };

  function createReminderRow(rem) {
    const curState = rem.state || 'pending';
    const row = document.createElement('div');
    row.className = `reminder-row status-${curState}`;

    const trigger = new Date(rem.remind_at);
    const timeFormatted = Number.isNaN(trigger.getTime())
      ? rem.remind_at
      : trigger.toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });

    const channelsHtml = (rem.channels || ['web'])
      .map((c) => (CHANNEL_META[c] ? `<span class="channel-badge" title="${CHANNEL_META[c].label}">${CHANNEL_META[c].icon} ${CHANNEL_META[c].label}</span>` : ''))
      .join('');

    const isDone = curState === 'acknowledged' || curState === 'dismissed';
    const statusLabel = curState.charAt(0).toUpperCase() + curState.slice(1);

    row.innerHTML = `
      <div class="reminder-info">
        <div class="reminder-title-line">
          <span class="reminder-title">${escapeHtml(rem.title)}</span>
          <span class="status-chip chip-${curState}">${statusLabel}</span>
        </div>
        ${rem.description ? `<p class="reminder-desc">${escapeHtml(rem.description)}</p>` : ''}
        <div class="reminder-timing">
          <span class="reminder-time">⏰ ${escapeHtml(timeFormatted)}</span>
          ${rem.is_recurring && rem.cron_expr ? `<span class="badge-recur" title="Cron">🔁 ${escapeHtml(rem.cron_expr)}</span>` : ''}
          ${rem.note_id ? '<span class="badge-recur">📎 linked note</span>' : ''}
        </div>
        <div class="reminder-channels">${channelsHtml}</div>
      </div>
      <div class="reminder-actions">
        ${!isDone ? `
          <button class="btn btn-glass btn-sm btn-rem-snooze" title="Snooze 15 minutes">Snooze 15m</button>
          <button class="btn btn-primary btn-sm btn-rem-ack" title="Mark done">Done</button>
        ` : ''}
        <button class="btn btn-glass btn-sm btn-rem-edit" title="Edit">Edit</button>
        <button class="btn btn-icon btn-rem-delete" title="Delete">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
        </button>
      </div>
    `;

    const snooze = row.querySelector('.btn-rem-snooze');
    if (snooze) snooze.onclick = () => snoozeReminder(rem.id, 15);
    const ack = row.querySelector('.btn-rem-ack');
    if (ack) ack.onclick = () => acknowledgeReminder(rem.id);
    row.querySelector('.btn-rem-edit').onclick = () => openReminderEditor(rem);
    row.querySelector('.btn-rem-delete').onclick = async () => {
      const ok = await confirmDialog(`Delete reminder "${rem.title}"?`);
      if (!ok) return;
      try { await apiJson(`/api/reminders/${rem.id}`, { method: 'DELETE' }); loadReminders(); }
      catch (err) { showToast('error', 'Delete failed', err.message); }
    };
    return row;
  }

  function openReminderEditor(reminder) {
    const re = state.reminderEditor;
    re.reminder = reminder;
    $('#reminder-modal-title').textContent = reminder ? 'Edit Reminder' : 'Schedule Reminder';
    $('#rem-title-input').value = reminder ? reminder.title : '';
    $('#rem-desc-input').value = reminder ? reminder.description || '' : '';
    $('#rem-time-input').value = reminder ? canonicalToLocalInput(reminder.remind_at) : formatDateTimeLocal(new Date(Date.now() + 3600000));

    const recur = reminder && reminder.is_recurring ? (reminder.recur_type || 'custom') : 'none';
    $('#rem-recur-select').value = recur;
    $('#rem-cron-input').value = (reminder && reminder.cron_expr) || '';
    $('#rem-custom-cron-group').classList.toggle('hidden', recur !== 'custom');

    const channels = (reminder && reminder.channels) || ['web'];
    $('#ch-email').checked = channels.includes('email');
    $('#ch-telegram').checked = channels.includes('telegram');
    $('#ch-email').disabled = !state.capabilities.email;
    $('#ch-telegram').disabled = !state.capabilities.telegram;
    $('#ch-email-note').textContent = state.capabilities.email ? '' : '(SMTP not configured)';
    $('#ch-telegram-note').textContent = state.capabilities.telegram ? '' : '(bot token not set)';

    populateNoteSelect(reminder && reminder.note_id);
    $('#modal-reminder').classList.remove('hidden');
    setTimeout(() => $('#rem-title-input').focus(), 30);
  }

  function closeReminderEditor() {
    $('#modal-reminder').classList.add('hidden');
    state.reminderEditor.reminder = null;
  }

  async function populateNoteSelect(selectedId) {
    const select = $('#rem-note-select');
    select.innerHTML = '<option value="">No note linked</option>';
    try {
      const data = await apiJson('/api/notes?limit=100');
      for (const note of data.notes || []) {
        const opt = document.createElement('option');
        opt.value = note.id;
        opt.textContent = note.title || 'Untitled';
        select.appendChild(opt);
      }
      if (selectedId) select.value = selectedId;
    } catch (_) {}
  }

  async function saveReminder() {
    const re = state.reminderEditor;
    const title = $('#rem-title-input').value.trim();
    if (!title) { showToast('error', 'Missing title', 'Give the reminder a title.'); return; }
    const localTime = $('#rem-time-input').value;
    if (!localTime) { showToast('error', 'Missing time', 'Pick a date and time.'); return; }
    const remind_at = localInputToIso(localTime);
    if (!remind_at) { showToast('error', 'Invalid time', 'Could not parse the date/time.'); return; }

    const recurOption = $('#rem-recur-select').value;
    let cronRule = null;
    if (recurOption === 'daily') cronRule = '0 9 * * *';
    else if (recurOption === 'weekly') cronRule = '0 9 * * 1';
    else if (recurOption === 'monthly') cronRule = '0 9 1 * *';
    else if (recurOption === 'custom') {
      cronRule = $('#rem-cron-input').value.trim();
      if (!cronRule) { showToast('error', 'Cron needed', 'Enter a cron expression (e.g. 0 9 * * 1-5).'); return; }
    }

    const channels = ['web'];
    if ($('#ch-email').checked) channels.push('email');
    if ($('#ch-telegram').checked) channels.push('telegram');

    const payload = {
      title,
      description: $('#rem-desc-input').value.trim() || null,
      remind_at,
      is_recurring: recurOption !== 'none',
      cron_expr: cronRule,
      recur_type: recurOption !== 'none' && recurOption !== 'custom' ? recurOption : null,
      note_id: $('#rem-note-select').value || null,
      channels,
    };

    const btn = $('#btn-save-reminder');
    btn.disabled = true;
    btn.textContent = 'Saving…';
    try {
      if (re.reminder) await apiJson(`/api/reminders/${re.reminder.id}`, { method: 'PUT', body: payload });
      else await apiJson('/api/reminders', { method: 'POST', body: payload });
      closeReminderEditor();
      await loadReminders();
      showToast('success', 'Reminder saved', title);
    } catch (err) {
      showToast('error', 'Save failed', err.message);
    } finally {
      btn.disabled = false;
      btn.textContent = 'Save Reminder';
    }
  }

  async function snoozeReminder(id, minutes) {
    try {
      await apiJson(`/api/reminders/${id}/snooze`, { method: 'POST', body: { duration_minutes: minutes } });
      await loadReminders();
      showToast('success', 'Snoozed', `Reminder pushed ${minutes} minutes.`);
    } catch (err) { showToast('error', 'Snooze failed', err.message); }
  }

  async function acknowledgeReminder(id) {
    try {
      await apiJson(`/api/reminders/${id}/acknowledge`, { method: 'POST', body: { action: 'acknowledge' } });
      await loadReminders();
    } catch (err) { showToast('error', 'Failed', err.message); }
  }

  // ---------------------------------------------------------------- calendar
  function renderCalendar() {
    const body = $('#calendar-body');
    const cursor = state.calendar.cursor;
    const reminders = state.reminders.filter((r) => r.state === 'pending' || r.state === 'snoozed');

    if (state.calendar.mode === 'month') {
      const year = cursor.getFullYear();
      const month = cursor.getMonth();
      $('#cal-title').textContent = cursor.toLocaleDateString([], { month: 'long', year: 'numeric' });
      const first = new Date(year, month, 1);
      const startOffset = (first.getDay() + 6) % 7; // Monday-first
      const daysInMonth = new Date(year, month + 1, 0).getDate();

      let html = '<div class="cal-grid cal-weekdays">';
      for (const d of ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun']) html += `<div class="cal-weekday">${d}</div>`;
      html += '</div><div class="cal-grid cal-month">';
      const today = new Date();
      for (let i = 0; i < startOffset; i++) html += '<div class="cal-day empty"></div>';
      for (let day = 1; day <= daysInMonth; day++) {
        const date = new Date(year, month, day);
        const isToday = date.toDateString() === today.toDateString();
        const dayReminders = reminders.filter((r) => {
          const d = new Date(r.remind_at);
          return d.getFullYear() === year && d.getMonth() === month && d.getDate() === day;
        });
        const chips = dayReminders
          .slice(0, 3)
          .map((r) => {
            const t = new Date(r.remind_at);
            const hhmm = t.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
            return `<div class="cal-event ${r.is_recurring ? 'recur' : ''}" title="${escapeHtml(r.title)} (${hhmm})">${escapeHtml(r.title.slice(0, 22))}<span class="cal-event-time">${hhmm}</span></div>`;
          })
          .join('');
        const more = dayReminders.length > 3 ? `<div class="cal-event-more">+${dayReminders.length - 3} more</div>` : '';
        html += `<div class="cal-day${isToday ? ' today' : ''}"><span class="cal-day-num">${day}</span>${chips}${more}</div>`;
      }
      html += '</div>';
      body.innerHTML = html;
    } else if (state.calendar.mode === 'week') {
      const monday = new Date(cursor);
      monday.setDate(cursor.getDate() - ((cursor.getDay() + 6) % 7));
      monday.setHours(0, 0, 0, 0);
      const sunday = new Date(monday);
      sunday.setDate(monday.getDate() + 6);
      $('#cal-title').textContent = `${monday.toLocaleDateString([], { month: 'short', day: 'numeric' })} – ${sunday.toLocaleDateString([], { month: 'short', day: 'numeric', year: 'numeric' })}`;
      const today = new Date();
      let html = '<div class="cal-grid cal-week">';
      for (let i = 0; i < 7; i++) {
        const day = new Date(monday);
        day.setDate(monday.getDate() + i);
        const isToday = day.toDateString() === today.toDateString();
        const dayReminders = reminders.filter((r) => {
          const d = new Date(r.remind_at);
          return d.toDateString() === day.toDateString();
        });
        const chips = dayReminders
          .map((r) => {
            const t = new Date(r.remind_at);
            const hhmm = t.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
            return `<div class="cal-event ${r.is_recurring ? 'recur' : ''}" title="${escapeHtml(r.title)}"><span class="cal-event-time">${hhmm}</span> ${escapeHtml(r.title.slice(0, 28))}</div>`;
          })
          .join('');
        html += `<div class="cal-week-col${isToday ? ' today' : ''}"><div class="cal-week-head">${day.toLocaleDateString([], { weekday: 'short' })} ${day.getDate()}</div>${chips}</div>`;
      }
      html += '</div>';
      body.innerHTML = html;
    } else {
      // Day view: hour-by-hour
      const day = new Date(cursor);
      day.setHours(0, 0, 0, 0);
      $('#cal-title').textContent = day.toLocaleDateString([], { weekday: 'long', month: 'long', day: 'numeric', year: 'numeric' });
      const today = new Date();
      const isToday = day.toDateString() === today.toDateString();
      let html = '<div class="cal-day-view">';
      for (let hour = 0; hour < 24; hour++) {
        const hourReminders = reminders.filter((r) => {
          const d = new Date(r.remind_at);
          return d.toDateString() === day.toDateString() && d.getHours() === hour;
        });
        const events = hourReminders
          .map((r) => {
            const t = new Date(r.remind_at);
            const hhmm = t.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
            return `<div class="cal-event ${r.is_recurring ? 'recur' : ''}"><span class="cal-event-time">${hhmm}</span> ${escapeHtml(r.title)}${r.description ? ` — <em>${escapeHtml(r.description.slice(0, 60))}</em>` : ''}</div>`;
          })
          .join('');
        const hourLabel = `${String(hour).padStart(2, '0')}:00`;
        const highlightHour = isToday && today.getHours() === hour;
        html += `<div class="cal-hour${highlightHour ? ' now' : ''}"><span class="cal-hour-label">${hourLabel}</span><div class="cal-hour-events">${events || (hourReminders.length ? '' : '<span class="cal-hour-empty"></span>')}</div></div>`;
      }
      html += '</div>';
      body.innerHTML = html;
    }
  }

  // ---------------------------------------------------------------- views
  function switchView(view) {
    state.view = view;
    $$('.nav-item').forEach((item) => item.classList.toggle('active', item.dataset.view === view));
    $$('.view-panel').forEach((panel) => {
      const on = panel.id === `view-${view}`;
      panel.classList.toggle('hidden', !on);
      panel.classList.toggle('active', on);
    });
    $('#filter-tabs').classList.toggle('hidden', view !== 'notes');
    closeMobileSidebar();
    if (view === 'reminders') loadReminders();
    else if (view === 'calendar') { loadReminders().then(renderCalendar); }
    else loadNotes();
  }

  function closeMobileSidebar() {
    $('#sidebar').classList.remove('open');
    $('#sidebar-backdrop').classList.add('hidden');
  }

  // ---------------------------------------------------------------- quick capture
  async function quickCapture(e) {
    const input = $('#input-quick-capture');
    const value = input.value.trim();
    if (!value) return;
    const wantMarkdown = e.ctrlKey || e.metaKey;
    if (wantMarkdown) {
      input.value = '';
      openNoteEditor(null, 'detailed');
      $('#note-title-input').value = value;
      $('#note-content-input').value = '';
      setTimeout(() => $('#note-content-input').focus(), 30);
      return;
    }
    try {
      await apiJson('/api/notes', {
        method: 'POST',
        body: { title: value, content: '', note_type: 'simple', color: null, icon: null, folder_id: state.folderFilter ? state.folderFilter.id : null, tags: [] },
      });
      input.value = '';
      await Promise.all([loadNotes(), loadTags(), loadFolders()]);
      showToast('success', 'Captured', value);
    } catch (err) {
      showToast('error', 'Capture failed', err.message);
    }
  }

  async function refreshAll() {
    await Promise.allSettled([loadNotes(), loadReminders(), loadTags(), loadFolders()]);
  }

  // ---------------------------------------------------------------- events
  function setupEventListeners() {
    // Quick add
    $('#btn-quick-simple').onclick = () => openNoteEditor(null, 'simple');
    $('#btn-quick-detailed').onclick = () => openNoteEditor(null, 'detailed');
    $('#btn-quick-code').onclick = () => openNoteEditor(null, 'code');
    $('#btn-quick-sheet').onclick = () => openNoteEditor(null, 'sheet');
    $('#btn-quick-reminder').onclick = () => openReminderEditor(null);

    // Nav
    $$('.nav-item').forEach((item) => (item.onclick = () => switchView(item.dataset.view)));

    // Mobile sidebar
    $('#btn-menu-toggle').onclick = () => {
      $('#sidebar').classList.toggle('open');
      $('#sidebar-backdrop').classList.toggle('hidden');
    };
    $('#sidebar-backdrop').onclick = closeMobileSidebar;

    // Auth
    $('#btn-request-otp').onclick = requestOtp;
    $('#btn-verify-otp').onclick = verifyOtp;
    $('#auth-email-input').addEventListener('keydown', (e) => { if (e.key === 'Enter') requestOtp(); });
    $('#auth-code-input').addEventListener('keydown', (e) => { if (e.key === 'Enter') verifyOtp(); });
    $('#btn-resend-otp').onclick = () => {
      $('#otp-step-2').classList.add('hidden');
      $('#otp-step-1').classList.remove('hidden');
      hideAuthError();
    };
    $('#btn-logout').onclick = logout;

    // Search
    const searchInput = $('#input-search');
    const clearBtn = $('#btn-clear-search');
    searchInput.oninput = debounce(() => {
      state.searchQuery = searchInput.value.trim();
      clearBtn.classList.toggle('hidden', !state.searchQuery);
      state.page = 1;
      if (state.view !== 'notes' && state.view !== 'archive') switchView('notes');
      loadNotes();
    }, 250);
    clearBtn.onclick = () => {
      searchInput.value = '';
      state.searchQuery = '';
      clearBtn.classList.add('hidden');
      state.page = 1;
      loadNotes();
      searchInput.focus();
    };

    // Search-in dropdown
    const searchInDropdown = $('#search-in-dropdown');
    $('#btn-search-in').onclick = (e) => {
      e.stopPropagation();
      searchInDropdown.classList.toggle('hidden');
    };
    $$('#search-in-dropdown input').forEach((cb) => {
      cb.onchange = () => {
        state.searchIn[cb.dataset.scope] = cb.checked;
        if (state.searchQuery) { state.page = 1; loadNotes(); }
      };
    });
    document.addEventListener('click', (e) => {
      if (!e.target.closest('.search-bar-wrapper')) searchInDropdown.classList.add('hidden');
    });

    // Filter tabs
    $$('#filter-tabs .tab-btn').forEach((tab) => {
      tab.onclick = () => {
        $$('#filter-tabs .tab-btn').forEach((t) => t.classList.remove('active'));
        tab.classList.add('active');
        state.noteFilter = tab.dataset.filter;
        state.page = 1;
        loadNotes();
      };
    });

    // Reminder filter chips
    $$('#reminder-filters .chip').forEach((chip) => {
      chip.onclick = () => {
        $$('#reminder-filters .chip').forEach((c) => c.classList.remove('active'));
        chip.classList.add('active');
        state.reminderFilter = chip.dataset.remFilter;
        renderReminders();
      };
    });
    $('#btn-new-reminder').onclick = () => openReminderEditor(null);

    // Pagination
    $('#btn-prev-page').onclick = () => { if (state.page > 1) { state.page--; loadNotes(); } };
    $('#btn-next-page').onclick = () => { state.page++; loadNotes(); };

    // Quick capture
    $('#input-quick-capture').addEventListener('keydown', (e) => { if (e.key === 'Enter') quickCapture(e); });

    // Note editor
    $('#btn-save-note').onclick = () => saveNote(false);
    $('#btn-cancel-note').onclick = closeNoteEditor;
    $('#btn-close-note').onclick = closeNoteEditor;
    $('#btn-delete-note').onclick = deleteActiveNote;
    $('#btn-pin-note').onclick = pinActiveNote;
    $('#btn-archive-note').onclick = archiveActiveNote;
    $('#btn-versions').onclick = openVersions;
    $('#btn-share-note').onclick = openShare;
    $('#btn-export-menu').onclick = (e) => { e.stopPropagation(); $('#export-dropdown').classList.toggle('hidden'); };
    $$('#export-dropdown button').forEach((b) => {
      b.onclick = () => { exportActiveNote(b.dataset.exportFormat); $('#export-dropdown').classList.add('hidden'); };
    });
    document.addEventListener('click', (e) => {
      if (!e.target.closest('.export-menu-wrap')) $('#export-dropdown').classList.add('hidden');
    });

    // Type segmented control
    $$('#editor-type-seg button').forEach((b) => {
      b.onclick = () => {
        const ed = state.editor;
        if (ed.type === 'sheet' && b.dataset.noteType !== 'sheet') {
          $('#note-content-input').value = sheetRowsToContent();
          ed.sheet = null;
        }
        ed.type = b.dataset.noteType;
        applyEditorType();
        markEditorDirty();
      };
    });

    // Title input
    $('#note-title-input').oninput = () => markEditorDirty();

    // Content input: live preview + autosave
    const contentInput = $('#note-content-input');
    const previewPane = $('#preview-pane');
    contentInput.oninput = debounce(() => {
      markEditorDirty();
      if (state.editor.type === 'detailed') {
        previewPane.innerHTML = renderMarkdownClient(contentInput.value);
        renderMermaidBlocks(previewPane);
      }
      if (state.editor.type === 'code') refreshCodePreview();
      renderSuggestedTags();
    }, 200);

    // Code language select
    $('#code-lang-select').onchange = () => { state.editor.lang = $('#code-lang-select').value; refreshCodePreview(); markEditorDirty(); };

    // Sheet toolbar
    $('#btn-sheet-paste').onclick = () => { $('#sheet-paste-input').value = ''; $('#modal-sheet-paste').classList.remove('hidden'); };
    $('#btn-sheet-add-row').onclick = sheetAddRow;
    $('#btn-sheet-add-col').onclick = sheetAddCol;
    $('#btn-sheet-del-row').onclick = sheetDelRow;
    $('#btn-sheet-del-col').onclick = sheetDelCol;
    $('#btn-apply-sheet-paste').onclick = () => {
      const text = $('#sheet-paste-input').value;
      if (!text.trim()) return;
      const rows = text.split(/\r?\n/).filter((l) => l.trim() !== '').map((line) => line.split('\t'));
      state.editor.sheet = rows.map((r) => r.map((c) => (c ?? '').trim()));
      renderSheetGrid();
      markEditorDirty();
      $('#modal-sheet-paste').classList.add('hidden');
      showToast('success', 'Imported', `${rows.length} rows imported as a sheet.`);
    };
    $('#btn-close-sheet-paste').onclick = () => $('#modal-sheet-paste').classList.add('hidden');
    $('#btn-cancel-sheet-paste').onclick = () => $('#modal-sheet-paste').classList.add('hidden');

    // Folder select in editor
    $('#note-folder-select').onchange = () => { state.editor.folderId = $('#note-folder-select').value; markEditorDirty(); };

    // Icon picker
    $('#btn-note-icon').onclick = (e) => {
      e.stopPropagation();
      openIconPicker($('#btn-note-icon'), (icon) => {
        state.editor.icon = icon;
        $('#btn-note-icon').textContent = icon || '🏷';
        markEditorDirty();
      }, state.editor.icon);
    };
    document.addEventListener('click', (e) => {
      if (!e.target.closest('.icon-picker-popover') && !e.target.closest('#btn-note-icon')) {
        $('#icon-picker-popover').classList.add('hidden');
      }
    });

    // Color picker
    $$('#color-picker-group .color-dot').forEach((dot) => {
      dot.onclick = () => {
        $$('#color-picker-group .color-dot').forEach((d) => d.classList.remove('active'));
        dot.classList.add('active');
        state.editor.color = dot.dataset.color;
        markEditorDirty();
      };
    });

    // Markdown toolbar
    $$('#markdown-toolbar .tool-btn').forEach((btn) => {
      btn.onclick = () => {
        const insert = btn.dataset.insert;
        const wrap = btn.dataset.wrap === 'true';
        const start = contentInput.selectionStart;
        const end = contentInput.selectionEnd;
        const text = contentInput.value;
        let out;
        if (wrap) {
          const selected = text.substring(start, end) || 'text';
          out = text.substring(0, start) + insert + selected + insert + text.substring(end);
        } else if (btn.dataset.line === 'true') {
          const lineStart = text.lastIndexOf('\n', start - 1) + 1;
          out = text.substring(0, lineStart) + insert + '\n' + text.substring(lineStart);
        } else {
          out = text.substring(0, start) + insert + text.substring(end);
        }
        contentInput.value = out;
        contentInput.focus();
        contentInput.dispatchEvent(new Event('input'));
      };
    });

    // Tag input
    const tagInput = $('#note-tags-input');
    tagInput.oninput = debounce(async () => {
      const q = tagInput.value.trim();
      if (!q) { hideTagSuggestDropdown(); return; }
      try {
        const data = await apiJson(`/api/tags/suggest?q=${encodeURIComponent(q)}`);
        showTagSuggestions(data.tags || [], q);
      } catch (_) { hideTagSuggestDropdown(); }
    }, 150);
    tagInput.onkeydown = (e) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        addEditorTag(tagInput.value);
      } else if (e.key === 'Backspace' && !tagInput.value && state.editor.tags.length) {
        state.editor.tags.pop();
        renderTagChips();
        markEditorDirty();
      } else if (e.key === 'Escape') {
        hideTagSuggestDropdown();
      }
    };
    document.addEventListener('click', (e) => {
      if (!e.target.closest('.tag-editor')) hideTagSuggestDropdown();
    });

    // Reminder modal
    $('#btn-save-reminder').onclick = saveReminder;
    $('#btn-cancel-reminder').onclick = closeReminderEditor;
    $('#btn-close-reminder').onclick = closeReminderEditor;
    $('#rem-recur-select').onchange = (e) => {
      $('#rem-custom-cron-group').classList.toggle('hidden', e.target.value !== 'custom');
    };

    // Folder modal
    $('#btn-new-folder').onclick = () => openFolderModal(null, null);
    $('#btn-save-folder').onclick = saveFolder;
    $('#btn-cancel-folder').onclick = closeFolderModal;
    $('#btn-close-folder').onclick = closeFolderModal;
    $('#btn-folder-icon').onclick = (e) => {
      e.stopPropagation();
      openIconPicker($('#btn-folder-icon'), (icon) => {
        state.folderEditor.icon = icon;
        $('#btn-folder-icon').innerHTML = `${icon || '🏷'} ${icon ? '' : 'None'}`;
      }, state.folderEditor.icon);
    };

    // Versions modal
    $('#btn-versions-close').onclick = () => $('#modal-versions').classList.add('hidden');
    $('#btn-close-versions').onclick = () => $('#modal-versions').classList.add('hidden');
    $('#btn-versions-clear').onclick = clearVersions;

    // Share modal
    $('#btn-share-live').onclick = () => createShare('live');
    $('#btn-share-snapshot').onclick = () => createShare('snapshot');
    $('#btn-close-share').onclick = () => $('#modal-share').classList.add('hidden');
    $('#btn-close-share2').onclick = () => $('#modal-share').classList.add('hidden');
    $('#btn-copy-share').onclick = async () => {
      const input = $('#share-link-input');
      input.select();
      try {
        await navigator.clipboard.writeText(input.value);
        showToast('success', 'Copied', 'Link copied to clipboard.');
      } catch (_) {
        document.execCommand('copy');
        showToast('success', 'Copied', 'Link copied to clipboard.');
      }
    };
    $('#btn-open-share').onclick = () => window.open($('#share-link-input').value, '_blank');
    $('#btn-revoke-share').onclick = revokeShare;

    // AI
    $('#btn-ai-toggle').onclick = () => {
      state.ai.enabled = !state.ai.enabled;
      updateAiToggleButton();
      renderAiHeader();
      renderAiEditorGroup();
      showToast('info', state.ai.enabled ? 'AI enabled' : 'AI disabled', state.ai.enabled ? `Model: ${state.ai.model}` : '');
    };
    $('#ai-model-select').onchange = (e) => { state.ai.model = e.target.value; };
    $('#ai-style-select').onchange = (e) => { state.ai.style = e.target.value; };
    $('#btn-ai-rewrite').onclick = aiRewrite;

    // Calendar controls
    $('#btn-cal-prev').onclick = () => {
      const c = state.calendar.cursor;
      if (state.calendar.mode === 'month') state.calendar.cursor = new Date(c.getFullYear(), c.getMonth() - 1, 1);
      else if (state.calendar.mode === 'week') state.calendar.cursor = new Date(c.getFullYear(), c.getMonth(), c.getDate() - 7);
      else state.calendar.cursor = new Date(c.getFullYear(), c.getMonth(), c.getDate() - 1);
      renderCalendar();
    };
    $('#btn-cal-next').onclick = () => {
      const c = state.calendar.cursor;
      if (state.calendar.mode === 'month') state.calendar.cursor = new Date(c.getFullYear(), c.getMonth() + 1, 1);
      else if (state.calendar.mode === 'week') state.calendar.cursor = new Date(c.getFullYear(), c.getMonth(), c.getDate() + 7);
      else state.calendar.cursor = new Date(c.getFullYear(), c.getMonth(), c.getDate() + 1);
      renderCalendar();
    };
    $('#btn-cal-today').onclick = () => { state.calendar.cursor = new Date(); renderCalendar(); };
    $$('[data-cal-mode]').forEach((b) => {
      b.onclick = () => {
        $$('[data-cal-mode]').forEach((x) => x.classList.remove('active'));
        b.classList.add('active');
        state.calendar.mode = b.dataset.calMode;
        renderCalendar();
      };
    });

    // Global keyboard shortcuts
    document.addEventListener('keydown', (e) => {
      if (e.key === 'Escape') {
        if (!$('#modal-note').classList.contains('hidden')) closeNoteEditor();
        else if (!$('#modal-reminder').classList.contains('hidden')) closeReminderEditor();
        else if (!$('#modal-folder').classList.contains('hidden')) closeFolderModal();
        else if (!$('#modal-versions').classList.contains('hidden')) $('#modal-versions').classList.add('hidden');
        else if (!$('#modal-share').classList.contains('hidden')) $('#modal-share').classList.add('hidden');
        else if (!$('#modal-sheet-paste').classList.contains('hidden')) $('#modal-sheet-paste').classList.add('hidden');
        else if (!$('#modal-confirm').classList.contains('hidden')) $('#btn-confirm-cancel').click();
        else hideTagSuggestDropdown();
      }

      const tag = document.activeElement && document.activeElement.tagName;
      const typing = tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT';

      if (e.key === '/' && !typing) {
        e.preventDefault();
        $('#input-search').focus();
      }

      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 's') {
        if (!$('#modal-note').classList.contains('hidden')) {
          e.preventDefault();
          saveNote(false);
        }
      }
    });

    // Leave note on window close
    window.addEventListener('beforeunload', () => {
      if (state.editor.note) {
        api(`/api/notes/${state.editor.note.id}/presence`, { method: 'DELETE', body: { device: state.deviceId } }).catch(() => {});
      }
    });
  }

  // ---------------------------------------------------------------- boot
  document.addEventListener('DOMContentLoaded', () => {
    // Seed the code language dropdown
    const LANGUAGES = [
      ['text', 'Plain text'], ['rust', 'Rust'], ['python', 'Python'], ['javascript', 'JavaScript'],
      ['typescript', 'TypeScript'], ['java', 'Java'], ['kotlin', 'Kotlin'], ['go', 'Go'],
      ['c', 'C'], ['cpp', 'C++'], ['csharp', 'C#'], ['ruby', 'Ruby'], ['php', 'PHP'],
      ['swift', 'Swift'], ['scala', 'Scala'], ['haskell', 'Haskell'], ['lua', 'Lua'],
      ['r', 'R'], ['dart', 'Dart'], ['elixir', 'Elixir'], ['clojure', 'Clojure'],
      ['sql', 'SQL'], ['bash', 'Bash / Shell'], ['powershell', 'PowerShell'], ['html', 'HTML'],
      ['css', 'CSS'], ['scss', 'SCSS'], ['json', 'JSON'], ['yaml', 'YAML'], ['toml', 'TOML'],
      ['xml', 'XML'], ['markdown', 'Markdown'], ['dockerfile', 'Dockerfile'], ['nginx', 'Nginx'],
      ['makefile', 'Makefile'], ['ini', 'INI'], ['diff', 'Diff'],
    ];
    $('#code-lang-select').innerHTML = LANGUAGES
      .map(([id, label]) => `<option value="${id}">${label}</option>`)
      .join('');

    setupEventListeners();
    checkAuthStatus();
  });
})();
