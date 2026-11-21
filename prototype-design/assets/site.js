/*
 * site.js — public website shell (header + footer) and the hero contour graphic.
 * Usage: <body data-nav="blog"> ... <div id="site-header"></div> ... <div id="site-footer"></div>
 * Load before proto.js.
 */
(function () {
  const NAV = [
    { k: 'blog', label: '文章', href: 'blog.html' },
    { k: 'projects', label: '项目', href: 'projects.html' },
    { k: 'tools', label: '工具', href: 'tools.html' },
    { k: 'ai-trending', label: 'AI 趋势', href: 'ai-trending.html' },
    { k: 'mcp', label: 'MCP 目录', href: 'mcp.html' },
    { k: 'about', label: '关于', href: 'about.html' },
  ];
  const active = document.body.dataset.nav;

  const header = document.getElementById('site-header');
  if (header) {
    header.outerHTML = `
    <a href="#main" class="sr-only focus:not-sr-only focus:fixed focus:top-2 focus:left-2 focus:z-[100] focus:bg-white focus:px-3 focus:py-2">跳到正文</a>
    <header class="sticky top-0 z-40 bg-paper/90 backdrop-blur border-b border-rule">
      <div class="max-w-site mx-auto px-4 md:px-8 h-16 flex items-center justify-between gap-6">
        <a href="home.html" class="flex items-baseline gap-2 shrink-0" aria-label="Silva 首页">
          <span class="font-serif text-[22px] font-bold tracking-tight text-ink">Silva</span>
          <span class="hidden sm:inline text-[13px] text-ink-soft">周屿的技术主页</span>
        </a>
        <nav aria-label="主导航" class="hidden md:block">
          <ul class="flex items-center gap-1 text-[15px]">
            ${NAV.map(n => `<li><a href="${n.href}" class="relative px-3 py-2 rounded ${n.k === active ? 'text-accent font-medium after:absolute after:left-3 after:right-3 after:-bottom-[13px] after:h-[2px] after:bg-accent' : 'text-ink-2 hover:text-ink'}" ${n.k === active ? 'aria-current="page"' : ''}>${n.label}</a></li>`).join('')}
          </ul>
        </nav>
        <button type="button" class="md:hidden -mr-2 p-2 text-ink" data-toggle="mobile-nav" aria-expanded="false" aria-controls="mobile-nav" aria-label="打开菜单">
          <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M4 7h16M4 12h16M4 17h16"/></svg>
        </button>
      </div>
      <nav id="mobile-nav" hidden aria-label="主导航（手机）" class="md:hidden border-t border-rule bg-paper">
        <ul class="px-4 py-2">
          ${NAV.map(n => `<li><a href="${n.href}" class="flex items-center justify-between py-3 border-b border-rule/70 last:border-0 text-[16px] ${n.k === active ? 'text-accent font-medium' : 'text-ink'}" ${n.k === active ? 'aria-current="page"' : ''}>${n.label}${n.k === active ? '<span class="text-xs">当前</span>' : ''}</a></li>`).join('')}
        </ul>
      </nav>
    </header>`;
  }

  const footer = document.getElementById('site-footer');
  if (footer) {
    footer.outerHTML = `
    <footer class="${document.body.hasAttribute('data-footer-flush') ? '' : 'mt-24'} border-t border-rule bg-paper-2/60">
      <div class="max-w-site mx-auto px-4 md:px-8 py-12 grid gap-10 md:grid-cols-12">
        <div class="md:col-span-5">
          <a href="home.html" class="font-serif text-xl font-bold text-ink">Silva</a>
          <p class="mt-3 text-[14px] leading-7 text-ink-soft max-w-sm">周屿的个人技术主页：记录后端与工程实践，整理做过的项目、常用工具，以及 AI 开源项目与 MCP Server 的观察。</p>
        </div>
        <nav aria-label="页脚栏目" class="md:col-span-4">
          <h2 class="text-[13px] text-ink-soft">栏目</h2>
          <ul class="mt-3 grid grid-cols-2 gap-y-2 text-[14px]">
            ${NAV.map(n => `<li><a class="text-ink-2 hover:text-accent" href="${n.href}">${n.label}</a></li>`).join('')}
          </ul>
        </nav>
        <div class="md:col-span-3">
          <h2 class="text-[13px] text-ink-soft">订阅与联络</h2>
          <ul class="mt-3 space-y-2 text-[14px]">
            <li><a class="text-ink-2 hover:text-accent inline-flex items-center gap-1.5" href="#rss"><svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="5" cy="19" r="2"/><path d="M4 11a9 9 0 0 1 9 9h-2.5A6.5 6.5 0 0 0 4 13.5zM4 4a16 16 0 0 1 16 16h-2.5A13.5 13.5 0 0 0 4 6.5z"/></svg>RSS 订阅</a></li>
            <li><a class="text-ink-2 hover:text-accent" href="https://github.com/" rel="noopener" target="_blank">GitHub <span class="text-ink-soft text-xs">（外部链接）</span></a></li>
            <li><a class="text-ink-2 hover:text-accent" href="mailto:hello@example.com">hello@example.com</a></li>
          </ul>
        </div>
      </div>
      <div class="border-t border-rule">
        <div class="max-w-site mx-auto px-4 md:px-8 py-5 text-[12px] text-ink-soft flex flex-wrap justify-between gap-2">
          <span>© 2026 周屿</span>
          <span class="tnum">站点版本 g-0142 · 数据快照 2026-11-20</span>
        </div>
      </div>
    </footer>`;
  }

  /* ---- Contour graphic: deterministic pseudo-topographic rings ---- */
  window.drawContours = function (svg, opts = {}) {
    if (!svg) return;
    const W = 600, H = 640;
    const cx = opts.cx ?? 330, cy = opts.cy ?? 300;
    const rings = opts.rings ?? 22;
    const seed = opts.seed ?? 7;
    const rand = (i) => { const x = Math.sin(i * 12.9898 + seed * 78.233) * 43758.5453; return x - Math.floor(x); };
    const ph = Array.from({ length: 6 }, (_, i) => rand(i + 1) * Math.PI * 2);
    let out = '';
    for (let r = 0; r < rings; r++) {
      const base = 18 + r * (opts.step ?? 17);
      const pts = [];
      for (let a = 0; a <= 360; a += 4) {
        const t = (a * Math.PI) / 180;
        const k = 1 + r / rings;
        const n = 0.16 * Math.sin(2 * t + ph[0] + r * 0.09) + 0.09 * Math.sin(3 * t + ph[1] - r * 0.05) * k
                + 0.05 * Math.sin(5 * t + ph[2] + r * 0.13) + 0.03 * Math.sin(7 * t + ph[3]);
        const rr = base * (1 + n);
        pts.push([cx + Math.cos(t) * rr * 1.08, cy + Math.sin(t) * rr * 0.92]);
      }
      const d = pts.map((p, i) => (i ? 'L' : 'M') + p[0].toFixed(1) + ' ' + p[1].toFixed(1)).join(' ') + 'Z';
      const hi = r === (opts.highlight ?? 9);
      out += `<path d="${d}" stroke="${hi ? '#27467A' : '#1D2330'}" stroke-width="${hi ? 2 : r % 5 === 4 ? 1.3 : 0.7}" stroke-opacity="${hi ? 1 : r % 5 === 4 ? 0.55 : 0.28}" style="--d:${(r * 0.045).toFixed(2)}s"/>`;
    }
    svg.setAttribute('viewBox', `0 0 ${W} ${H}`);
    svg.innerHTML = out;
    svg.querySelectorAll('path').forEach(p => p.style.setProperty('--len', Math.ceil(p.getTotalLength())));
    svg.classList.add('contour');
    if (opts.animate !== false) svg.classList.add('draw');
  };
})();
