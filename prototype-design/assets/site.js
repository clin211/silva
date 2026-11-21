/*
 * site.js — public website shell v3 ("The dark room").
 * Page contract:
 *   <head> … <script>document.documentElement.classList.add('js')</script> (before CSS paints)
 *   <body class="silva" data-nav="blog"> <div id="site-header"></div> <main id="main">…</main> <div id="site-footer"></div>
 *   scripts: ../assets/credits.js, ../assets/site.js, ../assets/proto.js
 * Behaviours provided:
 *   [data-rise]   children rise in sequence once the block enters the viewport (content visible without JS)
 *   [data-zoom]   picture settles from 1.06 → 1 when entering
 *   .drag-row[data-lit]  sideways row: mouse drag to scroll; the frame nearest the centre gets .lit,
 *                 its data-title is written into #<data-lit> caption element
 *   icon(name, cls) small inline SVG glyphs (arrows, external, close)
 */
(function () {
  document.documentElement.classList.add('js');
  const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;

  const G = {
    'arrow-right': '<path d="M5 12h14"/><path d="m12 5 7 7-7 7"/>',
    'arrow-left': '<path d="M19 12H5"/><path d="m12 19-7-7 7-7"/>',
    'arrow-up-right': '<path d="M7 7h10v10"/><path d="M7 17 17 7"/>',
    x: '<path d="M18 6 6 18"/><path d="m6 6 12 12"/>',
    menu: '<path d="M4 8h16"/><path d="M4 16h16"/>',
  };
  window.icon = (name, cls = '') =>
    `<svg class="${cls}" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="square" aria-hidden="true">${G[name] || ''}</svg>`;

  const NAV = [
    ['blog', '文章', 'blog.html'],
    ['projects', '项目', 'projects.html'],
    ['tools', '工具', 'tools.html'],
    ['ai-trending', 'AI 趋势', 'ai-trending.html'],
    ['mcp', 'MCP', 'mcp.html'],
    ['about', '关于', 'about.html'],
  ];
  const active = document.body.dataset.nav;

  /* ---------- header: wordmark, plain links ---------- */
  const header = document.getElementById('site-header');
  if (header) {
    header.outerHTML = `
    <a href="#main" class="skip" style="position:absolute;left:-9999px;top:8px;z-index:100;background:var(--accent);color:var(--accent-ink);padding:10px 14px" onfocus="this.style.left='8px'" onblur="this.style.left='-9999px'">跳到正文</a>
    <header class="site-head" style="position:relative;z-index:40">
      <div class="wrap" style="display:flex;align-items:center;justify-content:space-between;gap:24px;min-height:88px">
        <a href="home.html" class="t-lead" style="text-decoration:none;letter-spacing:-0.01em" aria-label="Silva 首页">Silva</a>
        <nav aria-label="主导航" class="nav-desk">
          <ul style="display:flex;gap:clamp(16px,2.2vw,36px);list-style:none;margin:0;padding:0">
            ${NAV.map(([k, l, h]) => `<li><a href="${h}" class="${k === active ? 'link ink' : 'muted'}" style="${k === active ? '' : 'text-decoration:none'};display:inline-block;padding:10px 0" ${k === active ? 'aria-current="page"' : ''}>${l}</a></li>`).join('')}
          </ul>
        </nav>
        <button type="button" class="nav-btn" aria-expanded="false" aria-controls="nav-sheet" style="display:none;min-height:44px;padding:0 4px;background:none;border:0;color:var(--ink);font:inherit;cursor:pointer">菜单</button>
      </div>
      <div id="nav-sheet" hidden style="position:fixed;inset:0;z-index:60;background:var(--bg);overflow:auto">
        <div class="wrap" style="display:flex;align-items:center;justify-content:space-between;min-height:88px">
          <a href="home.html" class="t-lead" style="text-decoration:none">Silva</a>
          <button type="button" class="nav-close" style="min-height:44px;background:none;border:0;color:var(--ink);font:inherit;cursor:pointer">关闭</button>
        </div>
        <nav aria-label="主导航（手机）" class="wrap"><ul style="list-style:none;margin:24px 0 0;padding:0">
          ${NAV.map(([k, l, h]) => `<li><a href="${h}" class="t-head" style="display:block;padding:10px 0;text-decoration:none;color:${k === active ? 'var(--accent)' : 'var(--ink)'}" ${k === active ? 'aria-current="page"' : ''}>${l}</a></li>`).join('')}
        </ul>
        <p class="t-small muted" style="margin-top:40px">后端工程师周屿的技术主页 · <a class="link" href="#rss">RSS 订阅</a></p></nav>
      </div>
    </header>
    <style>@media (max-width: 860px){ .nav-desk{display:none} .nav-btn{display:inline-flex !important;align-items:center} }</style>`;
    const btn = document.querySelector('.nav-btn'), sheet = document.getElementById('nav-sheet');
    const close = () => { sheet.hidden = true; btn.setAttribute('aria-expanded', 'false'); document.body.style.overflow = ''; btn.focus(); };
    btn.addEventListener('click', () => { sheet.hidden = false; btn.setAttribute('aria-expanded', 'true'); document.body.style.overflow = 'hidden'; sheet.querySelector('a[aria-current], nav a').focus(); });
    sheet.querySelector('.nav-close').addEventListener('click', close);
    document.addEventListener('keydown', e => { if (e.key === 'Escape' && !sheet.hidden) close(); });
  }

  /* ---------- footer: the last photograph is the footer ---------- */
  const footer = document.getElementById('site-footer');
  if (footer) {
    const credits = (window.SILVA_CREDITS || []).map(c =>
      `<li><a class="link" href="${c.page}" target="_blank" rel="noopener">${c.title}</a>，${c.creator}，${c.license}</li>`).join('');
    footer.outerHTML = `
    <footer class="site-foot" style="position:relative;margin-top:var(--gap-section);min-height:100svh;display:flex;flex-direction:column;justify-content:space-between;overflow:hidden;color:#fff">
      <video class="foot-video" autoplay muted loop playsinline preload="metadata" poster="img/traveltuesday-with-my-public-lands-23825649569-poster.jpg" aria-hidden="true"
        style="position:absolute;inset:0;width:100%;height:100%;object-fit:cover;filter:brightness(.62)">
        <source src="img/traveltuesday-with-my-public-lands-23825649569.webm" type="video/webm">
      </video>
      <div class="wrap" style="position:relative;width:100%;display:flex;flex-wrap:wrap;justify-content:space-between;gap:32px;padding-top:32px">
        <nav aria-label="页脚栏目"><ul style="list-style:none;margin:0;padding:0;display:grid;grid-template-columns:repeat(2,auto);gap:6px 40px">
          ${NAV.map(([, l, h]) => `<li><a class="link" href="${h}">${l}</a></li>`).join('')}
          <li><a class="link" href="#rss">RSS 订阅</a></li>
        </ul></nav>
        <address style="font-style:normal;text-align:right">
          <a class="link" href="mailto:hello@example.com">hello@example.com</a><br>
          <a class="link" href="https://github.com/" target="_blank" rel="noopener">GitHub @zhouyu</a><br>
          <span>浙江杭州</span>
        </address>
      </div>
      <div class="wrap" style="position:relative;width:100%;padding-bottom:28px">
        <p class="t-display" style="color:var(--accent);margin:0 0 20px -0.04em" aria-hidden="true">Silva</p>
        <div class="t-body" style="display:flex;flex-wrap:wrap;justify-content:space-between;gap:12px;color:#e9eee6">
          <span>© 2026 周屿 · 线上版本 g-0142 · 数据快照 2026-11-20</span>
          <details><summary style="cursor:pointer">图片与影像来源（${(window.SILVA_CREDITS || []).length}）</summary>
            <ol style="margin:12px 0 0;padding-left:1.2em;max-width:60em;display:grid;gap:4px">${credits}</ol></details>
        </div>
      </div>
    </footer>`;
    if (reduce) { const v = document.querySelector('.foot-video'); v && (v.removeAttribute('autoplay'), v.pause()); }
  }

  /* ---------- entrances ---------- */
  const io = new IntersectionObserver(es => es.forEach(e => {
    if (e.isIntersecting) { e.target.classList.add('in'); io.unobserve(e.target); }
  }), { rootMargin: '0px 0px -10% 0px' });
  window.observeRise = (root = document) => root.querySelectorAll('[data-rise], [data-zoom]').forEach(el => {
    if (el.dataset.rise !== undefined) [...el.children].forEach((c, i) => c.style.setProperty('--n', i));
    io.observe(el);
  });
  observeRise();

  /* ---------- drag rows ---------- */
  document.querySelectorAll('.drag-row').forEach(row => {
    let down = false, sx = 0, sl = 0, moved = false;
    row.addEventListener('pointerdown', e => { if (e.pointerType !== 'mouse') return; down = true; moved = false; sx = e.clientX; sl = row.scrollLeft; });
    addEventListener('pointermove', e => { if (!down) return; const dx = e.clientX - sx; if (Math.abs(dx) > 4) { moved = true; row.classList.add('dragging'); } row.scrollLeft = sl - dx; });
    addEventListener('pointerup', () => { if (!down) return; down = false; row.classList.remove('dragging'); });
    row.addEventListener('click', e => { if (moved) { e.preventDefault(); e.stopPropagation(); } }, true);
    const capId = row.dataset.lit;
    if (capId === undefined) return;
    const cap = capId ? document.getElementById(capId) : null;
    const frames = [...row.children];
    const lightUp = () => {
      const mid = row.getBoundingClientRect().left + row.clientWidth / 2;
      let best = frames[0], bd = Infinity;
      frames.forEach(f => { const r = f.getBoundingClientRect(); const d = Math.abs(r.left + r.width / 2 - mid); if (d < bd) { bd = d; best = f; } });
      frames.forEach(f => f.querySelector('.photo')?.classList.toggle('lit', f === best));
      if (cap && best.dataset.title) {
        cap.innerHTML = best.dataset.href ? `<a class="link" href="${best.dataset.href}">${best.dataset.title}</a>` : best.dataset.title;
        const r = best.getBoundingClientRect(), pr = cap.parentElement.getBoundingClientRect();
        cap.style.marginLeft = Math.max(0, Math.min(r.left - pr.left, pr.width - cap.offsetWidth)) + 'px';
      }
    };
    row.addEventListener('scroll', () => requestAnimationFrame(lightUp), { passive: true });
    addEventListener('resize', lightUp);
    if (row.dataset.start) { const s = frames[+row.dataset.start]; s && (row.scrollLeft = s.offsetLeft - (row.clientWidth - s.offsetWidth) / 2); }
    lightUp();
  });
})();
