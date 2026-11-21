/*
 * site.js — public website shell v4 ("明亮门户", after clin.pro).
 * Page contract:
 *   <head> … <script>document.documentElement.classList.add('js')</script>
 *   <body class="silva" data-nav="blog"> <div id="site-header"></div> <main id="main">…</main> <div id="site-footer"></div>
 *   <main class="flush"> when the first section sits under the floating header (home hero).
 *   scripts: ../assets/site.js, ../assets/proto.js
 * Behaviours provided:
 *   floating pill header with mobile dropdown; four-column footer
 *   [data-rise]   children rise in sequence once the block enters the viewport (content visible without JS)
 *   icon(name, cls) small inline SVG glyphs
 */
(function () {
  document.documentElement.classList.add('js');

  const G = {
    'arrow-right': '<path d="M5 12h14"/><path d="m12 5 7 7-7 7"/>',
    'arrow-left': '<path d="M19 12H5"/><path d="m12 19-7-7 7-7"/>',
    'arrow-up-right': '<path d="M7 7h10v10"/><path d="M7 17 17 7"/>',
    x: '<path d="M18 6 6 18"/><path d="m6 6 12 12"/>',
    menu: '<path d="M4 7h16"/><path d="M4 12h16"/><path d="M4 17h16"/>',
    rss: '<path d="M4 11a9 9 0 0 1 9 9"/><path d="M4 4a16 16 0 0 1 16 16"/><circle cx="5" cy="19" r="1"/>',
    github: '<path d="M15 22v-4a4.8 4.8 0 0 0-1-3.5c3 0 6-2 6-5.5.08-1.25-.27-2.48-1-3.5.28-1.15.28-2.35 0-3.5 0 0-1 0-3 1.5-2.64-.5-5.36-.5-8 0C6 2 5 2 5 2c-.3 1.15-.3 2.35 0 3.5A5.403 5.403 0 0 0 4 9c0 3.5 3 5.5 6 5.5-.39.49-.68 1.05-.85 1.65-.17.6-.22 1.23-.15 1.85v4"/><path d="M9 18c-4.51 2-5-2-7-2"/>',
    mail: '<rect x="3" y="5" width="18" height="14" rx="2"/><path d="m3 7 9 6 9-6"/>',
    pin: '<path d="M12 21s-7-6.2-7-11a7 7 0 0 1 14 0c0 4.8-7 11-7 11Z"/><circle cx="12" cy="10" r="2.5"/>',
    clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
    search: '<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>',
    star: '<path d="m12 3 2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1-4.4-4.3 6.1-.9Z"/>',
    link: '<path d="M10 13a5 5 0 0 0 7.5.5l3-3a5 5 0 0 0-7-7l-1.7 1.7"/><path d="M14 11a5 5 0 0 0-7.5-.5l-3 3a5 5 0 0 0 7 7l1.7-1.7"/>',
  };
  window.icon = (name, cls = '', size = 18) =>
    `<svg class="${cls}" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${G[name] || ''}</svg>`;

  const NAV = [
    ['home', '首页', 'home.html'],
    ['blog', '文章', 'blog.html'],
    ['ai-trending', 'AI 趋势', 'ai-trending.html'],
    ['about', '关于', 'about.html'],
  ];
  const active = document.body.dataset.nav;

  /* ---------- header: floating pill ---------- */
  const header = document.getElementById('site-header');
  if (header) {
    header.outerHTML = `
    <a href="#main" class="skip" style="position:fixed;left:-9999px;top:8px;z-index:100;background:var(--ink);color:#fff;padding:10px 14px;border-radius:999px" onfocus="this.style.left='8px'" onblur="this.style.left='-9999px'">跳到正文</a>
    <header class="site-head" style="position:fixed;inset:16px 0 auto;z-index:50;pointer-events:none">
      <div class="glass head-bar" style="pointer-events:auto;margin-inline:auto;height:58px;padding:0 10px 0 20px;border-radius:999px;display:flex;align-items:center;gap:12px">
        <a href="home.html" style="font-weight:700;font-size:1.125rem;letter-spacing:-.01em;display:flex;align-items:center;gap:8px;margin-right:auto" aria-label="Silva 首页">
          <span aria-hidden="true" style="display:inline-grid;place-items:center;width:28px;height:28px;border-radius:99px;background:var(--teal);color:#fff;font-size:.8125rem">S</span>Silva</a>
        <nav aria-label="主导航" class="nav-desk">
          <ul style="display:flex;gap:2px;list-style:none;margin:0;padding:0">
            ${NAV.map(([k, l, h]) => `<li><a href="${h}" class="nav-a" ${k === active ? 'aria-current="page"' : ''}>${l}</a></li>`).join('')}
          </ul>
        </nav>
        <a href="#rss" class="btn btn-dark btn-sm nav-rss" style="height:38px">${icon('rss', '', 16)}<span>RSS</span></a>
        <button type="button" class="nav-btn" aria-expanded="false" aria-controls="nav-sheet" aria-label="打开菜单">${icon('menu', '', 20)}</button>
      </div>
      <div id="nav-sheet" class="glass head-bar" hidden style="pointer-events:auto;margin:8px auto 0;border-radius:28px;padding:10px">
        <nav aria-label="主导航（手机）"><ul style="list-style:none;margin:0;padding:0;display:grid;grid-template-columns:1fr 1fr;gap:4px">
          ${NAV.map(([k, l, h]) => `<li><a href="${h}" class="nav-a" style="display:flex;height:48px;font-size:1rem" ${k === active ? 'aria-current="page"' : ''}>${l}</a></li>`).join('')}
        </ul></nav>
      </div>
    </header>
    <style>
      .head-bar{width:calc(100% - 2 * max(12px, var(--gutter) - 20px));max-width:calc(var(--content-max) + 40px)}
      /* frosted glass: translucent white, strong blur + saturation, light rim on top, soft shadow below */
      .glass{position:relative;isolation:isolate;
        background:linear-gradient(180deg,rgba(255,255,255,.62),rgba(255,255,255,.42));
        -webkit-backdrop-filter:blur(22px) saturate(1.8);backdrop-filter:blur(22px) saturate(1.8);
        box-shadow:inset 0 1px 0 rgba(255,255,255,.75),inset 0 0 0 1px rgba(255,255,255,.35),0 0 0 1px rgba(16,37,35,.06),0 8px 28px rgba(5,32,30,.08);
        transition:background .4s var(--ease),box-shadow .4s var(--ease)}
      .glass::after{content:"";position:absolute;inset:0;border-radius:inherit;pointer-events:none;z-index:-1;
        background:radial-gradient(120% 140% at 10% -20%,rgba(255,255,255,.55),transparent 55%)}
      .site-head.scrolled .glass{background:linear-gradient(180deg,rgba(255,255,255,.72),rgba(255,255,255,.55))}
      @supports not ((backdrop-filter:blur(1px)) or (-webkit-backdrop-filter:blur(1px))){.glass{background:rgba(255,255,255,.94)}}
      .nav-a{display:inline-flex;align-items:center;height:38px;padding:0 14px;border-radius:999px;font-size:.875rem;font-weight:500;color:rgba(16,37,35,.62);transition:color .3s,background-color .3s}
      .nav-a:hover{color:var(--ink);background:rgba(16,37,35,.05)}
      .nav-a[aria-current]{color:var(--ink);background:rgba(11,137,146,.1)}
      .nav-btn{display:none;width:40px;height:40px;border:0;border-radius:99px;background:rgba(16,37,35,.06);color:var(--ink);cursor:pointer;place-items:center}
      @media (max-width: 900px){ .nav-desk{display:none} .nav-btn{display:grid} }
      @media (max-width: 420px){ .nav-rss span{display:none} .nav-rss{width:38px;padding:0} }
    </style>`;
    const head = document.querySelector('.site-head');
    const onScroll = () => head.classList.toggle('scrolled', scrollY > 24);
    addEventListener('scroll', onScroll, { passive: true }); onScroll();
    const btn = document.querySelector('.nav-btn'), sheet = document.getElementById('nav-sheet');
    const set = open => { sheet.hidden = !open; btn.setAttribute('aria-expanded', String(open)); btn.innerHTML = icon(open ? 'x' : 'menu', '', 20); btn.setAttribute('aria-label', open ? '关闭菜单' : '打开菜单'); };
    btn.addEventListener('click', () => set(sheet.hidden));
    document.addEventListener('keydown', e => { if (e.key === 'Escape' && !sheet.hidden) { set(false); btn.focus(); } });
    document.addEventListener('click', e => { if (!sheet.hidden && !e.target.closest('.site-head')) set(false); });
  }

  /* ---------- footer: four columns ---------- */
  const footer = document.getElementById('site-footer');
  if (footer) {
    const col = (title, items) => `<div><h3 style="font-size:.9375rem;font-weight:600;margin:0 0 14px">${title}</h3>
      <ul style="list-style:none;margin:0;padding:0;display:grid;gap:10px">${items.map(([l, h]) => `<li><a class="t-small muted hover-accent" href="${h}">${l}</a></li>`).join('')}</ul></div>`;
    footer.outerHTML = `
    <footer class="site-foot band-paper" style="border-top:1px solid var(--line-soft);padding:64px 0 28px">
      <div class="wrap">
        <div style="display:grid;gap:40px" class="foot-grid">
          <div style="max-width:22rem">
            <p style="font-weight:700;font-size:1.25rem;color:var(--accent);margin:0">Silva · 长林啊</p>
            <p class="t-small muted" style="margin:12px 0 0">分享技术见解，记录学习心得。这里是技术文章和每天的 AI 开源观察，一点一点地长。</p>
            <div style="display:flex;gap:8px;margin-top:20px">
              <a href="https://github.com/clin211" target="_blank" rel="noopener" aria-label="GitHub（新窗口打开）" class="foot-ico">${icon('github', '', 18)}</a>
              <a href="mailto:hello@example.com" aria-label="发邮件" class="foot-ico">${icon('mail', '', 18)}</a>
              <a href="#rss" aria-label="RSS 订阅" class="foot-ico">${icon('rss', '', 18)}</a>
            </div>
          </div>
          ${col('内容', [['技术文章', 'blog.html'], ['后端', 'category.html'], ['AI 趋势', 'ai-trending.html']])}
          ${col('关于本站', [['关于长林啊', 'about.html'], ['RSS 订阅', '#rss'], ['站点地图', '#sitemap']])}
          <div>
            <h3 style="font-size:.9375rem;font-weight:600;margin:0 0 14px">联系方式</h3>
            <ul style="list-style:none;margin:0;padding:0;display:grid;gap:10px" class="t-small muted">
              <li style="display:flex;gap:8px;align-items:center">${icon('mail', '', 16)}<a class="hover-accent" href="mailto:hello@example.com">hello@example.com</a></li>
              <li style="display:flex;gap:8px;align-items:center">${icon('pin', '', 16)}四川，成都</li>
            </ul>
            <p class="t-small muted" style="margin:18px 0 0">新文章会同步到 RSS，不收集邮箱。</p>
          </div>
        </div>
        <div class="t-xs muted" style="margin-top:48px;padding-top:20px;border-top:1px solid var(--line-soft);display:flex;flex-wrap:wrap;justify-content:space-between;gap:8px">
          <span>© 2026 长林啊 · 保留所有权利</span>
          <span class="num">线上版本 g-0142 · 数据快照 2026-11-20</span>
        </div>
      </div>
    </footer>
    <style>
      @media (min-width: 768px){ .foot-grid{grid-template-columns:1.6fr 1fr 1fr 1.3fr} }
      .foot-ico{display:inline-grid;place-items:center;width:40px;height:40px;border-radius:99px;background:rgba(16,37,35,.05);color:var(--ink-2);transition:background-color .3s,color .3s}
      .foot-ico:hover{background:rgba(11,137,146,.12);color:var(--accent)}
    </style>`;
  }

  /* ---------- entrances ---------- */
  const io = new IntersectionObserver(es => es.forEach(e => {
    if (e.isIntersecting) { e.target.classList.add('in'); io.unobserve(e.target); }
  }), { rootMargin: '0px 0px -8% 0px' });
  window.observeRise = (root = document) => root.querySelectorAll('[data-rise]').forEach(el => {
    [...el.children].forEach((c, i) => c.style.setProperty('--n', i));
    io.observe(el);
  });
  observeRise();
})();
