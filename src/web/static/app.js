/**
 * evaNotes — Modern Single Page Application Controller
 * Handles dual authentication, notes CRUD with live markdown,
 * reminders scheduling & snooze/ack, and WebSocket live notifications.
 */

(function () {
  'use strict';

  // State
  const state = {
    user: null,
    currentView: 'notes-view', // 'notes-view' | 'reminders-view' | 'archived-view'
    noteFilter: 'all',         // 'all' | 'simple' | 'detailed' | 'pinned'
    reminderFilter: 'all',     // 'all' | 'pending' | 'snoozed' | 'acknowledged'
    searchQuery: '',
    currentPage: 1,
    pageSize: 24,
    totalNotes: 0,
    notes: [],
    reminders: [],
    
    // Editor State
    activeNote: null,          // null = creating new, object = editing existing
    editorType: 'simple',      // 'simple' | 'detailed'
    selectedColor: '',
    isPinned: false,
    isArchived: false,
    
    // Reminder Modal State
    activeReminder: null,
    
    // Auth State
    googleClientId: null,
    requestedEmail: '',
    
    // WebSocket
    ws: null,
    wsReconnectTimeout: null,
    wsReconnectAttempts: 0,
  };

  // Helper: DOM Selectors
  const $ = (selector) => document.querySelector(selector);
  const $$ = (selector) => document.querySelectorAll(selector);

  // Helper: Escape HTML
  function escapeHtml(str) {
    if (!str) return '';
    return str
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;')
      .replace(/'/g, '&#039;');
  }

  // Safe client-side Markdown rendering for live preview
  function renderMarkdownClient(md) {
    if (!md) return '';
    
    // Escape HTML first to prevent any script injections
    let safe = escapeHtml(md);

    // Code blocks: ```lang\ncode\n```
    safe = safe.replace(/```([a-zA-Z0-9_-]*)\n([\s\S]*?)```/g, function (_, lang, code) {
      return `<pre><code class="language-${lang}">${code.trim()}</code></pre>`;
    });

    // Inline code: `code`
    safe = safe.replace(/`([^`\n]+)`/g, '<code>$1</code>');

    // Headings: ### Title
    safe = safe.replace(/^### (.*$)/gim, '<h3>$1</h3>');
    safe = safe.replace(/^## (.*$)/gim, '<h2>$1</h2>');
    safe = safe.replace(/^# (.*$)/gim, '<h1>$1</h1>');

    // Blockquotes: > quote
    safe = safe.replace(/^\> (.*$)/gim, '<blockquote>$1</blockquote>');

    // Task list items: - [ ] or - [x]
    safe = safe.replace(/^\s*-\s+\[ \]\s+(.*$)/gim, '<li class="task-item"><input type="checkbox" disabled> $1</li>');
    safe = safe.replace(/^\s*-\s+\[x\]\s+(.*$)/gim, '<li class="task-item"><input type="checkbox" checked disabled> $1</li>');

    // Unordered lists: - item or * item
    safe = safe.replace(/^\s*[-\*]\s+(.*$)/gim, '<li>$1</li>');

    // Ordered lists: 1. item
    safe = safe.replace(/^\s*\d+\.\s+(.*$)/gim, '<li>$1</li>');

    // Bold: **text** or __text__
    safe = safe.replace(/\*\*(.*?)\*\*/g, '<strong>$1</strong>');
    safe = safe.replace(/__(.*?)__/g, '<strong>$1</strong>');

    // Italic: *text* or _text_
    safe = safe.replace(/\*(.*?)\*/g, '<em>$1</em>');
    safe = safe.replace(/_([^_]+)_/g, '<em>$1</em>');

    // Horizontal rule: ---
    safe = safe.replace(/^---$/gim, '<hr>');

    // Links: [text](url) — validate protocol to prevent javascript:
    safe = safe.replace(/\[([^\]]+)\]\((https?:\/\/[^\s\)]+)\)/g, '<a href="$2" target="_blank" rel="noopener noreferrer">$1</a>');

    // Wrap paragraphs: double newlines
    const paragraphs = safe.split(/\n{2,}/);
    return paragraphs
      .map(p => {
        const trimmed = p.trim();
        if (!trimmed) return '';
        if (/^<(h1|h2|h3|pre|blockquote|li|hr)/.test(trimmed)) {
          return trimmed;
        }
        return `<p>${trimmed.replace(/\n/g, '<br>')}</p>`;
      })
      .join('\n');
  }

  // Format Date utility
  function formatDate(isoStr) {
    if (!isoStr) return '';
    try {
      const d = new Date(isoStr);
      const now = new Date();
      const diffMs = now - d;
      const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24));
      
      if (diffDays === 0) {
        return 'Today at ' + d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
      } else if (diffDays === 1) {
        return 'Yesterday at ' + d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
      } else if (diffDays < 7) {
        return d.toLocaleDateString([], { weekday: 'short' }) + ' at ' + d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
      } else {
        return d.toLocaleDateString([], { month: 'short', day: 'numeric', year: d.getFullYear() !== now.getFullYear() ? 'numeric' : undefined });
      }
    } catch (_) {
      return isoStr;
    }
  }

  function formatDateTimeLocal(dateObj) {
    const pad = (n) => String(n).padStart(2, '0');
    return `${dateObj.getFullYear()}-${pad(dateObj.getMonth() + 1)}-${pad(dateObj.getDate())}T${pad(dateObj.getHours())}:${pad(dateObj.getMinutes())}`;
  }

  // --- API Client ---
  async function api(path, options = {}) {
    options.headers = options.headers || {};
    if (options.body && typeof options.body === 'object' && !(options.body instanceof FormData)) {
      options.headers['Content-Type'] = 'application/json';
      options.body = JSON.stringify(options.body);
    }
    options.credentials = 'same-origin';

    const res = await fetch(path, options);
    if (res.status === 401 && !path.startsWith('/api/auth/status')) {
      showAuthModal();
      throw new Error('Unauthorized');
    }
    return res;
  }

  // --- Authentication ---
  async function checkAuthStatus() {
    try {
      const res = await api('/api/auth/status');
      const data = await res.json();
      state.googleClientId = data.google_client_id || null;

      if (data.google_auth_enabled) {
        $('#google-auth-container').classList.remove('hidden');
        initGoogleSignIn();
      } else {
        $('#google-auth-container').classList.add('hidden');
      }

      if (data.authenticated) {
        await fetchCurrentUser();
      } else {
        // Try /api/auth/me just in case
        try {
          const meRes = await api('/api/auth/me');
          if (meRes.ok) {
            const user = await meRes.json();
            onLoginSuccess(user);
          } else {
            showAuthModal();
          }
        } catch (_) {
          showAuthModal();
        }
      }
    } catch (err) {
      console.error('Failed to check auth status:', err);
      showAuthModal();
    }
  }

  async function fetchCurrentUser() {
    try {
      const res = await api('/api/auth/me');
      if (res.ok) {
        const user = await res.json();
        onLoginSuccess(user);
      } else {
        showAuthModal();
      }
    } catch (_) {
      showAuthModal();
    }
  }

  function onLoginSuccess(user) {
    state.user = user;
    hideAuthModal();

    // Update user badge
    const email = user.email || 'user@example.com';
    $('#user-email-display').textContent = email;
    $('#user-role-display').textContent = user.role ? user.role.toUpperCase() : 'USER';
    $('#user-avatar-initials').textContent = (email[0] || 'U').toUpperCase();

    // Init WebSocket
    initWebSocket();

    // Load initial data
    loadNotes();
    loadReminders();

    // Request browser notification permissions if supported
    if ('Notification' in window && Notification.permission === 'default') {
      Notification.requestPermission();
    }
  }

  function showAuthModal() {
    $('#modal-auth').classList.remove('hidden');
    $('#otp-step-1').classList.remove('hidden');
    $('#otp-step-2').classList.add('hidden');
    $('#auth-error-msg').classList.add('hidden');
  }

  function hideAuthModal() {
    $('#modal-auth').classList.add('hidden');
    $('#auth-error-msg').classList.add('hidden');
  }

  function showAuthError(msg) {
    const errBox = $('#auth-error-msg');
    errBox.textContent = msg;
    errBox.classList.remove('hidden');
  }

  async function requestOtp() {
    const emailInput = $('#auth-email-input');
    const email = emailInput.value.trim();
    if (!email) {
      showAuthError('Please enter an authorized admin email address.');
      return;
    }

    state.requestedEmail = email;
    const btn = $('#btn-request-otp');
    btn.disabled = true;
    btn.textContent = 'Sending code...';
    $('#auth-error-msg').classList.add('hidden');

    try {
      const res = await api('/api/auth/otp/request', {
        method: 'POST',
        body: { email }
      });
      const data = await res.json();
      if (res.ok) {
        $('#otp-step-1').classList.add('hidden');
        $('#otp-step-2').classList.remove('hidden');
        $('#auth-code-input').value = '';
        $('#auth-code-input').focus();
      } else {
        showAuthError(data.error || 'Failed to request OTP code.');
      }
    } catch (err) {
      showAuthError(err.message || 'Network error requesting OTP.');
    } finally {
      btn.disabled = false;
      btn.textContent = 'Send Verification Code';
    }
  }

  async function verifyOtp() {
    const code = $('#auth-code-input').value.trim();
    if (!code || code.length !== 6) {
      showAuthError('Please enter the 6-digit verification code.');
      return;
    }

    const btn = $('#btn-verify-otp');
    btn.disabled = true;
    btn.textContent = 'Verifying...';
    $('#auth-error-msg').classList.add('hidden');

    try {
      const res = await api('/api/auth/otp/verify', {
        method: 'POST',
        body: {
          email: state.requestedEmail,
          code: code
        }
      });
      const data = await res.json();
      if (res.ok) {
        onLoginSuccess(data.user);
      } else {
        showAuthError(data.error || 'Invalid or expired code.');
      }
    } catch (err) {
      showAuthError(err.message || 'Verification failed.');
    } finally {
      btn.disabled = false;
      btn.textContent = 'Sign In';
    }
  }

  async function logout() {
    try {
      await api('/api/auth/logout', { method: 'POST' });
    } catch (_) {}
    state.user = null;
    if (state.ws) {
      state.ws.close();
      state.ws = null;
    }
    showAuthModal();
  }

  function initGoogleSignIn() {
    if (!window.google || !state.googleClientId) return;

    window.handleGoogleCredentialResponse = async function (response) {
      try {
        const res = await api('/api/auth/google', {
          method: 'POST',
          body: { id_token: response.credential }
        });
        const data = await res.json();
        if (res.ok) {
          onLoginSuccess(data.user);
        } else {
          showAuthError(data.error || 'Google login failed.');
        }
      } catch (err) {
        showAuthError('Failed to verify Google token.');
      }
    };

    window.google.accounts.id.initialize({
      client_id: state.googleClientId,
      callback: window.handleGoogleCredentialResponse,
    });
    window.google.accounts.id.renderButton(
      document.querySelector('.g_id_signin'),
      { theme: 'filled_black', size: 'large', width: '380' }
    );
  }

  // --- WebSocket Connection ---
  function initWebSocket() {
    if (state.ws && (state.ws.readyState === WebSocket.OPEN || state.ws.readyState === WebSocket.CONNECTING)) {
      return;
    }

    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${protocol}//${window.location.host}/ws`;

    const indicator = $('#ws-indicator');
    const statusText = indicator.querySelector('.status-text');

    try {
      state.ws = new WebSocket(wsUrl);

      state.ws.onopen = function () {
        indicator.classList.remove('disconnected');
        indicator.classList.add('connected');
        statusText.textContent = 'Connected';
        state.wsReconnectAttempts = 0;
      };

      state.ws.onmessage = function (event) {
        try {
          const msg = JSON.parse(event.data);
          handleWebSocketMessage(msg);
        } catch (err) {
          console.warn('Malformed WS message:', event.data);
        }
      };

      state.ws.onclose = function () {
        indicator.classList.remove('connected');
        indicator.classList.add('disconnected');
        statusText.textContent = 'Reconnecting...';
        scheduleWsReconnect();
      };

      state.ws.onerror = function () {
        state.ws.close();
      };
    } catch (e) {
      scheduleWsReconnect();
    }
  }

  function scheduleWsReconnect() {
    if (state.wsReconnectTimeout) clearTimeout(state.wsReconnectTimeout);
    const delay = Math.min(30000, 1000 * Math.pow(1.5, state.wsReconnectAttempts++));
    state.wsReconnectTimeout = setTimeout(() => {
      if (state.user) initWebSocket();
    }, delay);
  }

  function handleWebSocketMessage(msg) {
    if (msg.event === 'reminder_trigger' || msg.type === 'reminder') {
      const data = msg.data || msg;
      showReminderToast(data);
      loadReminders(); // Refresh list & badge
    } else if (msg.type === 'pong') {
      // heart-beat response
    }
  }

  // Toast Notification
  function showReminderToast(data) {
    const container = $('#toast-container');
    const toast = document.createElement('div');
    toast.className = 'toast';
    
    const title = escapeHtml(data.title || 'Scheduled Reminder');
    const desc = escapeHtml(data.description || 'It is time for your scheduled reminder!');
    const remId = data.id || '';

    toast.innerHTML = `
      <div class="toast-header">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="12" cy="12" r="10"></circle>
          <polyline points="12 6 12 12 16 14"></polyline>
        </svg>
        <span>${title}</span>
      </div>
      <div class="toast-body">${desc}</div>
      <div class="toast-actions">
        ${remId ? `<button class="btn btn-glass btn-sm btn-toast-snooze" data-id="${remId}">Snooze 10m</button>` : ''}
        ${remId ? `<button class="btn btn-primary btn-sm btn-toast-ack" data-id="${remId}">Done</button>` : ''}
        <button class="btn btn-glass btn-sm btn-toast-dismiss">Dismiss</button>
      </div>
    `;

    toast.querySelector('.btn-toast-dismiss').onclick = () => toast.remove();
    
    const snoozeBtn = toast.querySelector('.btn-toast-snooze');
    if (snoozeBtn) {
      snoozeBtn.onclick = async () => {
        await snoozeReminder(remId, 10);
        toast.remove();
      };
    }

    const ackBtn = toast.querySelector('.btn-toast-ack');
    if (ackBtn) {
      ackBtn.onclick = async () => {
        await acknowledgeReminder(remId);
        toast.remove();
      };
    }

    container.appendChild(toast);

    // Desktop Notification if authorized
    if ('Notification' in window && Notification.permission === 'granted') {
      new Notification(`evaNotes: ${data.title || 'Reminder'}`, {
        body: data.description || 'Your reminder is due.',
        icon: '/favicon.ico'
      });
    }

    // Auto dismiss after 20 seconds
    setTimeout(() => {
      if (toast.parentNode) toast.remove();
    }, 20000);
  }

  // --- Notes Controller ---
  async function loadNotes() {
    try {
      const isArchived = state.currentView === 'archived-view';
      let url = `/api/notes?limit=${state.pageSize}&offset=${(state.currentPage - 1) * state.pageSize}&is_archived=${isArchived}`;
      
      if (state.noteFilter === 'simple' || state.noteFilter === 'detailed') {
        url += `&note_type=${state.noteFilter}`;
      } else if (state.noteFilter === 'pinned') {
        url += `&is_pinned=true`;
      }

      if (state.searchQuery) {
        url += `&search=${encodeURIComponent(state.searchQuery)}`;
      }

      const res = await api(url);
      if (!res.ok) return;

      const data = await res.json();
      state.notes = data.notes || [];
      state.totalNotes = data.total || 0;

      renderNotesList();
      updateNotesBadge();
    } catch (err) {
      console.error('Failed to load notes:', err);
    }
  }

  function updateNotesBadge() {
    $('#badge-total-notes').textContent = state.totalNotes;
  }

  function renderNotesList() {
    const isArchived = state.currentView === 'archived-view';
    const grid = isArchived ? $('#archived-grid') : $('#notes-grid');
    const emptyState = $('#notes-empty-state');
    const pagination = $('#notes-pagination');

    grid.innerHTML = '';

    if (state.notes.length === 0) {
      if (!isArchived) emptyState.classList.remove('hidden');
      pagination.classList.add('hidden');
      return;
    }

    emptyState.classList.add('hidden');

    state.notes.forEach(note => {
      const card = createNoteCard(note);
      grid.appendChild(card);
    });

    // Pagination update
    const totalPages = Math.ceil(state.totalNotes / state.pageSize) || 1;
    if (totalPages > 1) {
      pagination.classList.remove('hidden');
      $('#pagination-info').textContent = `Page ${state.currentPage} of ${totalPages}`;
      $('#btn-prev-page').disabled = state.currentPage <= 1;
      $('#btn-next-page').disabled = state.currentPage >= totalPages;
    } else {
      pagination.classList.add('hidden');
    }
  }

  function createNoteCard(note) {
    const card = document.createElement('div');
    card.className = `note-card ${note.is_pinned ? 'pinned' : ''}`;
    if (note.color) {
      card.style.borderLeft = `4px solid ${note.color}`;
    }

    const typeLabel = note.note_type === 'detailed' ? 'Markdown' : 'Simple';
    const typeBadgeClass = note.note_type === 'detailed' ? 'badge-detailed' : 'badge-simple';

    // Strip basic markdown syntax for card snippet
    const snippet = escapeHtml(note.content.substring(0, 220)) || '<em>No additional content</em>';

    card.innerHTML = `
      <div class="note-card-header">
        <h4 class="note-card-title">${escapeHtml(note.title || 'Untitled Note')}</h4>
        <span class="type-badge ${typeBadgeClass}">${typeLabel}</span>
      </div>
      <div class="note-card-snippet">${snippet}</div>
      <div class="note-card-footer">
        <span>${formatDate(note.updated_at || note.created_at)}</span>
        <div class="card-footer-actions">
          ${note.is_pinned ? '<span title="Pinned">📌</span>' : ''}
          ${note.is_archived ? '<span title="Archived">📦</span>' : ''}
        </div>
      </div>
    `;

    card.onclick = () => openNoteModal('edit', note);
    return card;
  }

  // Note Modal Editor
  function openNoteModal(mode, note = null) {
    const modal = $('#modal-note');
    state.activeNote = note;

    const titleInput = $('#note-title-input');
    const contentInput = $('#note-content-input');
    const previewPane = $('#preview-pane');
    const typeBadge = $('#editor-type-badge');
    const toolbar = $('#markdown-toolbar');
    const btnDelete = $('#btn-delete-note');
    const btnPin = $('#btn-pin-note');
    const btnArchive = $('#btn-archive-note');
    const btnExport = $('#btn-export-md');

    if (mode === 'edit' && note) {
      state.editorType = note.note_type;
      state.selectedColor = note.color || '';
      state.isPinned = note.is_pinned;
      state.isArchived = note.is_archived;

      titleInput.value = note.title;
      contentInput.value = note.content;
      btnDelete.classList.remove('hidden');
      btnExport.classList.remove('hidden');
    } else {
      state.editorType = mode === 'detailed' ? 'detailed' : 'simple';
      state.selectedColor = '';
      state.isPinned = false;
      state.isArchived = false;

      titleInput.value = '';
      contentInput.value = '';
      btnDelete.classList.add('hidden');
      btnExport.classList.add('hidden');
    }

    // Type badge & toolbar
    if (state.editorType === 'detailed') {
      typeBadge.textContent = 'Markdown Note';
      typeBadge.className = 'type-badge badge-detailed';
      toolbar.classList.remove('hidden');
      previewPane.classList.remove('hidden');
      previewPane.innerHTML = renderMarkdownClient(contentInput.value);
    } else {
      typeBadge.textContent = 'Simple Note';
      typeBadge.className = 'type-badge badge-simple';
      toolbar.classList.add('hidden');
      previewPane.classList.add('hidden');
    }

    // Buttons label state
    btnPin.textContent = state.isPinned ? '📌 Unpin' : '📌 Pin';
    btnArchive.textContent = state.isArchived ? 'Unarchive' : 'Archive';

    // Color picker active dot
    $$('.color-dot').forEach(dot => {
      dot.classList.toggle('active', dot.dataset.color === state.selectedColor);
    });

    modal.classList.remove('hidden');
    titleInput.focus();
  }

  function closeNoteModal() {
    $('#modal-note').classList.add('hidden');
    state.activeNote = null;
  }

  async function saveNote() {
    const title = $('#note-title-input').value.trim() || 'Untitled Note';
    const content = $('#note-content-input').value;
    const btnSave = $('#btn-save-note');

    btnSave.disabled = true;
    btnSave.textContent = 'Saving...';

    const payload = {
      title,
      content,
      note_type: state.editorType,
      color: state.selectedColor || null,
      is_pinned: state.isPinned,
      is_archived: state.isArchived
    };

    try {
      if (state.activeNote) {
        // Update
        const res = await api(`/api/notes/${state.activeNote.id}`, {
          method: 'PUT',
          body: payload
        });
        if (!res.ok) throw new Error('Failed to update note');
      } else {
        // Create
        const res = await api('/api/notes', {
          method: 'POST',
          body: payload
        });
        if (!res.ok) throw new Error('Failed to create note');
      }

      closeNoteModal();
      loadNotes();
    } catch (err) {
      alert(err.message || 'Error saving note.');
    } finally {
      btnSave.disabled = false;
      btnSave.textContent = 'Save Note';
    }
  }

  async function deleteActiveNote() {
    if (!state.activeNote) return;
    if (!confirm('Are you sure you want to delete this note?')) return;

    try {
      const res = await api(`/api/notes/${state.activeNote.id}`, { method: 'DELETE' });
      if (res.ok) {
        closeNoteModal();
        loadNotes();
      } else {
        alert('Failed to delete note.');
      }
    } catch (err) {
      alert('Error deleting note.');
    }
  }

  async function toggleActivePin() {
    state.isPinned = !state.isPinned;
    $('#btn-pin-note').textContent = state.isPinned ? '📌 Unpin' : '📌 Pin';
    if (state.activeNote) {
      await api(`/api/notes/${state.activeNote.id}/pin`, { method: 'PUT' });
      loadNotes();
    }
  }

  async function toggleActiveArchive() {
    state.isArchived = !state.isArchived;
    $('#btn-archive-note').textContent = state.isArchived ? 'Unarchive' : 'Archive';
    if (state.activeNote) {
      await api(`/api/notes/${state.activeNote.id}/archive`, { method: 'PUT' });
      closeNoteModal();
      loadNotes();
    }
  }

  function exportActiveNote() {
    if (!state.activeNote) return;
    window.location.href = `/api/notes/${state.activeNote.id}/export`;
  }

  // --- Reminders Controller ---
  async function loadReminders() {
    try {
      const res = await api('/api/reminders');
      if (!res.ok) return;

      state.reminders = await res.json();
      renderRemindersList();
      updateRemindersBadge();
      populateNoteSelectDropdown();
    } catch (err) {
      console.error('Failed to load reminders:', err);
    }
  }

  function updateRemindersBadge() {
    const pendingCount = state.reminders.filter(r => r.status === 'pending' || r.status === 'snoozed').length;
    const badge = $('#badge-pending-reminders');
    badge.textContent = pendingCount;
    badge.classList.toggle('badge-alert', pendingCount > 0);
  }

  function renderRemindersList() {
    const list = $('#reminders-list');
    const emptyState = $('#reminders-empty-state');
    list.innerHTML = '';

    const filtered = state.reminders.filter(r => {
      if (state.reminderFilter === 'all') return true;
      return r.status === state.reminderFilter;
    });

    if (filtered.length === 0) {
      emptyState.classList.remove('hidden');
      return;
    }

    emptyState.classList.add('hidden');

    filtered.forEach(rem => {
      const row = createReminderRow(rem);
      list.appendChild(row);
    });
  }

  function createReminderRow(rem) {
    const row = document.createElement('div');
    row.className = `reminder-row status-${rem.status}`;

    const triggerDate = new Date(rem.trigger_at);
    const timeFormatted = triggerDate.toLocaleString([], {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit'
    });

    const isRecur = !!rem.recurrence_rule;
    const statusLabel = rem.status.charAt(0).toUpperCase() + rem.status.slice(1);

    row.innerHTML = `
      <div class="reminder-info">
        <span class="reminder-title">${escapeHtml(rem.title)}</span>
        ${rem.description ? `<p class="reminder-desc">${escapeHtml(rem.description)}</p>` : ''}
        <div class="reminder-timing">
          <span>⏰ ${timeFormatted}</span>
          ${isRecur ? `<span class="badge-recur">🔁 ${escapeHtml(rem.recurrence_rule)}</span>` : ''}
          <span class="status-chip chip-${rem.status}">${statusLabel}</span>
        </div>
      </div>
      <div class="reminder-actions">
        ${rem.status !== 'acknowledged' ? `
          <button class="btn btn-glass btn-sm btn-rem-snooze" title="Snooze 15 minutes">Snooze 15m</button>
          <button class="btn btn-primary btn-sm btn-rem-ack" title="Mark Acknowledged">Done</button>
        ` : ''}
        <button class="btn btn-icon btn-rem-delete" title="Delete Reminder">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <polyline points="3 6 5 6 21 6"></polyline>
            <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path>
          </svg>
        </button>
      </div>
    `;

    const snoozeBtn = row.querySelector('.btn-rem-snooze');
    if (snoozeBtn) {
      snoozeBtn.onclick = () => snoozeReminder(rem.id, 15);
    }

    const ackBtn = row.querySelector('.btn-rem-ack');
    if (ackBtn) {
      ackBtn.onclick = () => acknowledgeReminder(rem.id);
    }

    row.querySelector('.btn-rem-delete').onclick = () => deleteReminder(rem.id);

    return row;
  }

  function populateNoteSelectDropdown() {
    const select = $('#rem-note-select');
    if (!select) return;
    
    // Preserve default
    select.innerHTML = '<option value="">No note linked</option>';
    state.notes.forEach(note => {
      const opt = document.createElement('option');
      opt.value = note.id;
      opt.textContent = note.title || 'Untitled Note';
      select.appendChild(opt);
    });
  }

  function openReminderModal() {
    const modal = $('#modal-reminder');
    $('#rem-title-input').value = '';
    $('#rem-desc-input').value = '';
    
    // Default time: 1 hour in the future
    const defaultTime = new Date(Date.now() + 60 * 60 * 1000);
    $('#rem-time-input').value = formatDateTimeLocal(defaultTime);
    
    $('#rem-recur-select').value = 'none';
    $('#rem-custom-cron-group').classList.add('hidden');
    $('#rem-cron-input').value = '';

    modal.classList.remove('hidden');
    $('#rem-title-input').focus();
  }

  function closeReminderModal() {
    $('#modal-reminder').classList.add('hidden');
  }

  async function saveReminder() {
    const title = $('#rem-title-input').value.trim();
    if (!title) {
      alert('Please enter a reminder title.');
      return;
    }

    const timeVal = $('#rem-time-input').value;
    if (!timeVal) {
      alert('Please select a trigger date and time.');
      return;
    }

    const triggerAt = new Date(timeVal).toISOString();
    const recurOption = $('#rem-recur-select').value;
    let cronRule = null;

    if (recurOption === 'daily') {
      cronRule = '0 9 * * *';
    } else if (recurOption === 'weekly') {
      cronRule = '0 9 * * 1';
    } else if (recurOption === 'monthly') {
      cronRule = '0 9 1 * *';
    } else if (recurOption === 'custom') {
      cronRule = $('#rem-cron-input').value.trim();
      if (!cronRule) {
        alert('Please specify the custom cron expression.');
        return;
      }
    }

    const noteId = $('#rem-note-select').value || null;
    const description = $('#rem-desc-input').value.trim() || null;

    const payload = {
      title,
      description,
      trigger_at: triggerAt,
      recurrence_rule: cronRule,
      note_id: noteId
    };

    const btn = $('#btn-save-reminder');
    btn.disabled = true;
    btn.textContent = 'Saving...';

    try {
      const res = await api('/api/reminders', {
        method: 'POST',
        body: payload
      });
      if (!res.ok) throw new Error('Failed to create reminder');

      closeReminderModal();
      loadReminders();
    } catch (err) {
      alert(err.message || 'Failed to save reminder.');
    } finally {
      btn.disabled = false;
      btn.textContent = 'Save Reminder';
    }
  }

  async function snoozeReminder(id, minutes) {
    try {
      const res = await api(`/api/reminders/${id}/snooze`, {
        method: 'POST',
        body: { minutes }
      });
      if (res.ok) {
        loadReminders();
      }
    } catch (err) {
      console.error('Failed to snooze reminder:', err);
    }
  }

  async function acknowledgeReminder(id) {
    try {
      const res = await api(`/api/reminders/${id}/acknowledge`, { method: 'POST' });
      if (res.ok) {
        loadReminders();
      }
    } catch (err) {
      console.error('Failed to acknowledge reminder:', err);
    }
  }

  async function deleteReminder(id) {
    if (!confirm('Are you sure you want to delete this reminder?')) return;
    try {
      const res = await api(`/api/reminders/${id}`, { method: 'DELETE' });
      if (res.ok) {
        loadReminders();
      }
    } catch (err) {
      console.error('Failed to delete reminder:', err);
    }
  }

  // --- View Switcher ---
  function switchView(viewId) {
    state.currentView = viewId;

    $$('.nav-item').forEach(item => {
      item.classList.toggle('active', item.dataset.view === viewId);
    });

    $$('.view-panel').forEach(panel => {
      panel.classList.toggle('hidden', panel.id !== viewId);
      panel.classList.toggle('active', panel.id === viewId);
    });

    if (viewId === 'notes-view') {
      $('#notes-view-title').textContent = 'All Notes';
      loadNotes();
    } else if (viewId === 'archived-view') {
      loadNotes();
    } else if (viewId === 'reminders-view') {
      loadReminders();
    }
  }

  // --- Event Listeners Setup ---
  function setupEventListeners() {
    // Quick Add
    $('#btn-quick-simple').onclick = () => openNoteModal('simple');
    $('#btn-quick-detailed').onclick = () => openNoteModal('detailed');
    $('#btn-quick-reminder').onclick = () => openReminderModal();

    // Nav Items
    $$('.nav-item').forEach(item => {
      item.onclick = () => switchView(item.dataset.view);
    });

    // Auth actions
    $('#btn-request-otp').onclick = requestOtp;
    $('#btn-verify-otp').onclick = verifyOtp;
    $('#btn-resend-otp').onclick = () => {
      $('#otp-step-2').classList.add('hidden');
      $('#otp-step-1').classList.remove('hidden');
      $('#auth-error-msg').classList.add('hidden');
    };
    $('#btn-logout').onclick = logout;

    // Search bar
    const searchInput = $('#input-search');
    const clearBtn = $('#btn-clear-search');

    let searchTimeout;
    searchInput.oninput = (e) => {
      const val = e.target.value.trim();
      clearBtn.classList.toggle('hidden', !val);
      clearTimeout(searchTimeout);
      searchTimeout = setTimeout(() => {
        state.searchQuery = val;
        state.currentPage = 1;
        loadNotes();
      }, 300);
    };

    clearBtn.onclick = () => {
      searchInput.value = '';
      clearBtn.classList.add('hidden');
      state.searchQuery = '';
      state.currentPage = 1;
      loadNotes();
    };

    // Filter Tabs
    $$('.tab-btn').forEach(tab => {
      tab.onclick = () => {
        $$('.tab-btn').forEach(t => t.classList.remove('active'));
        tab.classList.add('active');
        state.noteFilter = tab.dataset.filter;
        state.currentPage = 1;
        loadNotes();
      };
    });

    // Reminder Filter Chips
    $$('.chip[data-rem-filter]').forEach(chip => {
      chip.onclick = () => {
        $$('.chip[data-rem-filter]').forEach(c => c.classList.remove('active'));
        chip.classList.add('active');
        state.reminderFilter = chip.dataset.remFilter;
        renderRemindersList();
      };
    });

    // Pagination
    $('#btn-prev-page').onclick = () => {
      if (state.currentPage > 1) {
        state.currentPage--;
        loadNotes();
      }
    };
    $('#btn-next-page').onclick = () => {
      state.currentPage++;
      loadNotes();
    };

    // Note Editor Events
    $('#btn-save-note').onclick = saveNote;
    $('#btn-delete-note').onclick = deleteActiveNote;
    $('#btn-pin-note').onclick = toggleActivePin;
    $('#btn-archive-note').onclick = toggleActiveArchive;
    $('#btn-export-md').onclick = exportActiveNote;

    // Live markdown preview sync
    const contentInput = $('#note-content-input');
    const previewPane = $('#preview-pane');
    contentInput.oninput = () => {
      if (state.editorType === 'detailed') {
        previewPane.innerHTML = renderMarkdownClient(contentInput.value);
      }
    };

    // Color picker
    $$('.color-dot').forEach(dot => {
      dot.onclick = () => {
        $$('.color-dot').forEach(d => d.classList.remove('active'));
        dot.classList.add('active');
        state.selectedColor = dot.dataset.color;
      };
    });

    // Markdown Toolbar insertion
    $$('.tool-btn').forEach(btn => {
      btn.onclick = () => {
        const insertText = btn.dataset.insert;
        const wrap = btn.dataset.wrap === 'true';
        const start = contentInput.selectionStart;
        const end = contentInput.selectionEnd;
        const text = contentInput.value;

        if (wrap) {
          const selected = text.substring(start, end);
          contentInput.value = text.substring(0, start) + insertText + selected + insertText + text.substring(end);
          contentInput.focus();
          contentInput.selectionStart = start + insertText.length;
          contentInput.selectionEnd = end + insertText.length;
        } else {
          contentInput.value = text.substring(0, start) + insertText + text.substring(end);
          contentInput.focus();
          contentInput.selectionStart = contentInput.selectionEnd = start + insertText.length;
        }

        if (state.editorType === 'detailed') {
          previewPane.innerHTML = renderMarkdownClient(contentInput.value);
        }
      };
    });

    // Reminder modal events
    $('#btn-save-reminder').onclick = saveReminder;
    $('#rem-recur-select').onchange = (e) => {
      $('#rem-custom-cron-group').classList.toggle('hidden', e.target.value !== 'custom');
    };

    // Keyboard Shortcuts
    document.addEventListener('keydown', (e) => {
      // Escape to close modals
      if (e.key === 'Escape') {
        closeNoteModal();
        closeReminderModal();
      }

      // Quick Search '/'
      if (e.key === '/' && document.activeElement.tagName !== 'INPUT' && document.activeElement.tagName !== 'TEXTAREA') {
        e.preventDefault();
        searchInput.focus();
      }

      // Cmd/Ctrl + S in note editor
      if ((e.metaKey || e.ctrlKey) && e.key === 's') {
        if (!$('#modal-note').classList.contains('hidden')) {
          e.preventDefault();
          saveNote();
        }
      }
    });
  }

  // Expose global app methods for inline HTML onclick handlers
  window.app = {
    openNoteModal,
    closeNoteModal,
    openReminderModal,
    closeReminderModal,
    switchView,
  };

  // Bootstrap
  document.addEventListener('DOMContentLoaded', () => {
    setupEventListeners();
    checkAuthStatus();
  });
})();
