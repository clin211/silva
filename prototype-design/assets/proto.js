/*
 * proto.js — shared prototype behaviours for every page.
 *
 * 1) Prototype state switcher (not product UI):
 *    <body data-proto-states="default:正常;empty:暂无数据;error:请求失败">
 *    Elements with data-state="empty error" are shown only in those states.
 *    Current state also readable via ?state=xxx so states can be linked.
 * 2) Admin role switcher: <body data-proto-roles>  (author | editor | admin)
 *    Elements with data-role="editor" visible for editor+admin, data-role="admin" for admin only,
 *    data-role-only="author" visible only for that role. Also ?role=xxx.
 * 3) Overlays: [data-open="id"] opens #id (drawer/dialog with data-overlay). [data-close] closes.
 *    Esc closes, focus returns to the trigger.
 * 4) Tabs: [data-tabs] container; buttons [data-tab="k"], panels [data-panel="k"] (same container).
 * 5) Toast: any element with data-toast="文案" shows a toast on click.
 * 6) Disclosure: [data-toggle="id"] toggles #id hidden + aria-expanded.
 */
(function () {
  const qs = new URLSearchParams(location.search);
  const body = document.body;
  const ROLE_RANK = { author: 1, editor: 2, admin: 3 };
  const P = (window.Proto = { state: null, role: null });

  /* ---------- states ---------- */
  const states = (body.dataset.protoStates || '')
    .split(';').map(s => s.trim()).filter(Boolean)
    .map(s => { const [k, label] = s.split(':'); return { k, label: label || k }; });

  function applyState(k) {
    P.state = k;
    document.querySelectorAll('[data-state]').forEach(el => {
      el.hidden = !el.dataset.state.split(/\s+/).includes(k);
    });
    document.querySelectorAll('[data-proto-state-btn]').forEach(b => {
      b.setAttribute('aria-pressed', b.dataset.protoStateBtn === k);
    });
    document.dispatchEvent(new CustomEvent('proto:state', { detail: k }));
  }

  /* ---------- roles ---------- */
  const hasRoles = body.hasAttribute('data-proto-roles');
  function applyRole(r) {
    P.role = r;
    document.querySelectorAll('[data-role]').forEach(el => {
      el.hidden = ROLE_RANK[r] < ROLE_RANK[el.dataset.role];
    });
    document.querySelectorAll('[data-role-only]').forEach(el => {
      el.hidden = !el.dataset.roleOnly.split(/\s+/).includes(r);
    });
    document.querySelectorAll('[data-proto-role-btn]').forEach(b => {
      b.setAttribute('aria-pressed', b.dataset.protoRoleBtn === r);
    });
    document.querySelectorAll('[data-current-role]').forEach(el => {
      el.textContent = { author: 'Author', editor: 'Editor', admin: 'Admin' }[r];
    });
    document.dispatchEvent(new CustomEvent('proto:role', { detail: r }));
  }

  /* ---------- prototype bar ---------- */
  function buildBar() {
    const root = body.dataset.protoRoot || '../';
    const bar = document.createElement('div');
    bar.id = 'proto-bar';
    bar.className = 'fixed bottom-3 left-3 z-[90] text-[12px] text-white' + (body.dataset.adminNav ? ' lg:left-[236px]' : '');
    const btn = (attr, k, label) =>
      `<button type="button" ${attr}="${k}" class="px-2 py-1 rounded aria-pressed:bg-white aria-pressed:text-[#1D2330] hover:bg-white/15">${label}</button>`;
    bar.innerHTML = `
      <div class="bg-[#1D2330]/95 shadow-lg rounded-lg p-1.5 flex flex-col gap-1 max-w-[calc(100vw-24px)]">
        <div id="proto-panel" class="flex flex-col gap-1" ${qs.get('bar') === '0' ? 'hidden' : ''}>
          ${states.length > 1 ? `<div class="flex flex-wrap items-center gap-1"><span class="opacity-60 px-1">页面状态</span>${states.map(s => btn('data-proto-state-btn', s.k, s.label)).join('')}</div>` : ''}
          ${hasRoles ? `<div class="flex flex-wrap items-center gap-1"><span class="opacity-60 px-1">当前角色</span>${btn('data-proto-role-btn', 'author', 'Author')}${btn('data-proto-role-btn', 'editor', 'Editor')}${btn('data-proto-role-btn', 'admin', 'Admin')}</div>` : ''}
        </div>
        <div class="flex items-center gap-1">
          <a href="${root}index.html" class="px-2 py-1 rounded hover:bg-white/15">原型目录</a>
          ${states.length > 1 || hasRoles ? `<button type="button" id="proto-collapse" class="px-2 py-1 rounded hover:bg-white/15" aria-controls="proto-panel">收起</button>` : ''}
        </div>
      </div>`;
    body.appendChild(bar);
    bar.addEventListener('click', e => {
      const s = e.target.closest('[data-proto-state-btn]');
      const r = e.target.closest('[data-proto-role-btn]');
      if (s) applyState(s.dataset.protoStateBtn);
      if (r) applyRole(r.dataset.protoRoleBtn);
      if (e.target.id === 'proto-collapse') {
        const p = bar.querySelector('#proto-panel');
        p.hidden = !p.hidden;
        e.target.textContent = p.hidden ? '状态' : '收起';
      }
    });
  }

  /* ---------- overlays ---------- */
  let lastTrigger = null;
  function openOverlay(id, trigger) {
    const el = document.getElementById(id);
    if (!el) return;
    lastTrigger = trigger || document.activeElement;
    el.hidden = false;
    const f = el.querySelector('[autofocus], input, select, textarea, button:not([data-close])') || el.querySelector('button');
    f && setTimeout(() => f.focus(), 20);
  }
  function closeOverlay(el) {
    if (!el) return;
    el.hidden = true;
    lastTrigger && lastTrigger.focus && lastTrigger.focus();
  }
  P.open = openOverlay;
  P.close = id => closeOverlay(document.getElementById(id));

  /* ---------- toast ---------- */
  function toast(msg, tone) {
    let wrap = document.getElementById('toast-wrap');
    if (!wrap) {
      wrap = document.createElement('div');
      wrap.id = 'toast-wrap';
      wrap.setAttribute('role', 'status');
      wrap.setAttribute('aria-live', 'polite');
      wrap.className = 'fixed top-4 left-1/2 -translate-x-1/2 z-[95] flex flex-col gap-2 items-center';
      body.appendChild(wrap);
    }
    const t = document.createElement('div');
    const color = tone === 'bad' ? 'bg-[#A3261F]' : tone === 'ok' ? 'bg-[#1F6B4A]' : 'bg-[#1D2330]';
    t.className = `${color} text-white text-sm px-4 py-2.5 rounded-md shadow-lg`;
    t.textContent = msg;
    wrap.appendChild(t);
    setTimeout(() => t.remove(), 2600);
  }
  P.toast = toast;

  document.addEventListener('click', e => {
    const o = e.target.closest('[data-open]');
    if (o) { e.preventDefault(); openOverlay(o.dataset.open, o); }
    const c = e.target.closest('[data-close]');
    if (c) { e.preventDefault(); closeOverlay(c.closest('[data-overlay]')); }
    if (e.target.matches('[data-overlay-backdrop]')) closeOverlay(e.target.closest('[data-overlay]'));
    const t = e.target.closest('[data-toast]');
    if (t) toast(t.dataset.toast, t.dataset.toastTone);
    const tg = e.target.closest('[data-toggle]');
    if (tg) {
      const el = document.getElementById(tg.dataset.toggle);
      if (el) { el.hidden = !el.hidden; tg.setAttribute('aria-expanded', !el.hidden); }
    }
    const tab = e.target.closest('[data-tab]');
    if (tab) {
      const box = tab.closest('[data-tabs]');
      box.querySelectorAll('[data-tab]').forEach(b => {
        const on = b === tab;
        b.setAttribute('aria-selected', on);
      });
      box.querySelectorAll('[data-panel]').forEach(p => { p.hidden = p.dataset.panel !== tab.dataset.tab; });
    }
  });
  document.addEventListener('keydown', e => {
    if (e.key !== 'Escape') return;
    const open = [...document.querySelectorAll('[data-overlay]')].filter(el => !el.hidden).pop();
    if (open) closeOverlay(open);
  });

  /* ---------- init (after shells injected) ---------- */
  function init() {
    if (qs.get('bar') !== '0') buildBar();
    if (states.length) applyState(states.some(s => s.k === qs.get('state')) ? qs.get('state') : states[0].k);
    if (hasRoles) applyRole(ROLE_RANK[qs.get('role')] ? qs.get('role') : (body.dataset.protoRoles || 'admin'));
  }
  P.setState = applyState; P.setRole = applyRole;
  P.reapply = () => { if (P.state) applyState(P.state); if (P.role) applyRole(P.role); };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init);
  else init();
})();
