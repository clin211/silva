/*
 * site.js — public website shell v2 (Bento / monochrome + blue).
 * Usage: <body class="site" data-nav="blog"> <div id="site-header"></div> ... <div id="site-footer"></div>
 * Icons: <i data-icon="github" class="w-4 h-4"></i> is replaced with an inline Lucide SVG (aria-hidden).
 *        In JS: icon('arrow-right', 'w-4 h-4') returns the SVG string; hydrateIcons(root) re-runs replacement.
 * Load before proto.js.
 */
(function () {
  /* ---------- Lucide icon subset (24×24, stroke 2) ---------- */
  const I = {
    'arrow-right': '<path d="M5 12h14"/><path d="m12 5 7 7-7 7"/>',
    'arrow-left': '<path d="M19 12H5"/><path d="m12 19-7-7 7-7"/>',
    'arrow-up-right': '<path d="M7 7h10v10"/><path d="M7 17 17 7"/>',
    'external-link': '<path d="M15 3h6v6"/><path d="M10 14 21 3"/><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>',
    github: '<path d="M15 22v-4a4.8 4.8 0 0 0-1-3.5c3 0 6-2 6-5.5.08-1.25-.27-2.48-1-3.5.28-1.15.28-2.35 0-3.5 0 0-1 0-3 1.5-2.64-.5-5.36-.5-8 0C6 2 5 2 5 2c-.3 1.15-.3 2.35 0 3.5A5.403 5.403 0 0 0 4 9c0 3.5 3 5.5 6 5.5-.39.49-.68 1.05-.85 1.650-.17.6-.22 1.23-.15 1.85v4"/><path d="M9 18c-4.51 2-5-2-7-2"/>',
    rss: '<path d="M4 11a9 9 0 0 1 9 9"/><path d="M4 4a16 16 0 0 1 16 16"/><circle cx="5" cy="19" r="1"/>',
    mail: '<rect width="20" height="16" x="2" y="4" rx="2"/><path d="m22 7-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 7"/>',
    menu: '<path d="M4 6h16"/><path d="M4 12h16"/><path d="M4 18h16"/>',
    x: '<path d="M18 6 6 18"/><path d="m6 6 12 12"/>',
    'map-pin': '<path d="M20 10c0 4.993-5.539 10.193-7.399 11.799a1 1 0 0 1-1.202 0C9.539 20.193 4 14.993 4 10a8 8 0 0 1 16 0"/><circle cx="12" cy="10" r="3"/>',
    star: '<path d="M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.679a2.123 2.123 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.123 2.123 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.122 2.122 0 0 0-1.973 0L6.396 21.01a.53.53 0 0 1-.77-.56l.881-5.139a2.122 2.122 0 0 0-.611-1.879L2.16 9.795a.53.53 0 0 1 .294-.906l5.165-.755a2.122 2.122 0 0 0 1.597-1.16z"/>',
    'trending-up': '<path d="M22 7 13.5 15.5 8.5 10.5 2 17"/><path d="M16 7h6v6"/>',
    'trending-down': '<path d="M22 17 13.5 8.5 8.5 13.5 2 7"/><path d="M16 17h6v-6"/>',
    'git-fork': '<circle cx="12" cy="18" r="3"/><circle cx="6" cy="6" r="3"/><circle cx="18" cy="6" r="3"/><path d="M18 9v2c0 .6-.4 1-1 1H7c-.6 0-1-.4-1-1V9"/><path d="M12 12v3"/>',
    'book-open': '<path d="M12 7v14"/><path d="M3 18a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h5a4 4 0 0 1 4 4 4 4 0 0 1 4-4h5a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-6a3 3 0 0 0-3 3 3 3 0 0 0-3-3z"/>',
    briefcase: '<path d="M16 20V4a2 2 0 0 0-2-2h-4a2 2 0 0 0-2 2v16"/><rect width="20" height="14" x="2" y="6" rx="2"/>',
    wrench: '<path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/>',
    plug: '<path d="M12 22v-5"/><path d="M9 8V2"/><path d="M15 8V2"/><path d="M18 8v5a4 4 0 0 1-4 4h-4a4 4 0 0 1-4-4V8Z"/>',
    calendar: '<path d="M8 2v4"/><path d="M16 2v4"/><rect width="18" height="18" x="3" y="4" rx="2"/><path d="M3 10h18"/>',
    clock: '<circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/>',
    terminal: '<path d="m4 17 6-6-6-6"/><path d="M12 19h8"/>',
    code: '<path d="m16 18 6-6-6-6"/><path d="m8 6-6 6 6 6"/>',
    tag: '<path d="M12.586 2.586A2 2 0 0 0 11.172 2H4a2 2 0 0 0-2 2v7.172a2 2 0 0 0 .586 1.414l8.704 8.704a2.426 2.426 0 0 0 3.42 0l6.58-6.58a2.426 2.426 0 0 0 0-3.420z"/><circle cx="7.5" cy="7.5" r=".5" fill="currentColor"/>',
    folder: '<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>',
    'chevron-down': '<path d="m6 9 6 6 6-6"/>',
    'chevron-left': '<path d="m15 18-6-6 6-6"/>',
    'chevron-right': '<path d="m9 18 6-6-6-6"/>',
    'alert-circle': '<circle cx="12" cy="12" r="10"/><path d="M12 8v4"/><path d="M12 16h.01"/>',
    'refresh-cw': '<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/><path d="M8 16H3v5"/>',
    'check-circle': '<path d="M21.801 10A10 10 0 1 1 17 3.335"/><path d="m9 11 3 3L22 4"/>',
    info: '<circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/>',
    list: '<path d="M3 12h.01"/><path d="M3 18h.01"/><path d="M3 6h.01"/><path d="M8 12h13"/><path d="M8 18h13"/><path d="M8 6h13"/>',
    home: '<path d="M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8"/><path d="M3 10a2 2 0 0 1 .709-1.528l7-5.999a2 2 0 0 1 2.582 0l7 5.999A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
    user: '<path d="M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/>',
    sparkles: '<path d="M9.937 15.5A2 2 0 0 0 8.5 14.063l-6.135-1.582a.5.5 0 0 1 0-.962L8.5 9.936A2 2 0 0 0 9.937 8.5l1.582-6.135a.5.5 0 0 1 .963 0L14.063 8.5A2 2 0 0 0 15.5 9.937l6.135 1.581a.5.5 0 0 1 0 .964L15.5 14.063a2 2 0 0 0-1.437 1.437l-1.582 6.135a.5.5 0 0 1-.963 0z"/>',
    filter: '<path d="M22 3H2l8 9.46V19l4 2v-8.54L22 3z"/>',
    'circle-dot': '<circle cx="12" cy="12" r="10"/><circle cx="12" cy="12" r="1"/>',
    layers: '<path d="m12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83Z"/><path d="m22 17.65-9.17 4.16a2 2 0 0 1-1.66 0L2 17.65"/><path d="m22 12.65-9.17 4.16a2 2 0 0 1-1.66 0L2 12.65"/>',
    hash: '<path d="M4 9h16"/><path d="M4 15h16"/><path d="M10 3 8 21"/><path d="M16 3l-2 18"/>',
    scale: '<path d="m16 16 3-8 3 8c-.87.65-1.92 1-3 1s-2.13-.35-3-1Z"/><path d="m2 16 3-8 3 8c-.87.65-1.920 1-3 1s-2.13-.35-3-1Z"/><path d="M7 21h10"/><path d="M12 3v18"/><path d="M3 7h2c2 0 5-1 7-2 2 1 5 2 7 2h2"/>',
  };
  const icon = (window.icon = (name, cls = 'w-4 h-4') =>
    `<svg class="${cls} shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${I[name] || ''}</svg>`);
  window.hydrateIcons = (root = document) =>
    root.querySelectorAll('i[data-icon]').forEach(el => { el.outerHTML = icon(el.dataset.icon, el.className || 'w-4 h-4'); });

  const NAV = [
    { k: 'blog', label: '文章', href: 'blog.html' },
    { k: 'projects', label: '项目', href: 'projects.html' },
    { k: 'tools', label: '工具', href: 'tools.html' },
    { k: 'ai-trending', label: 'AI 趋势', href: 'ai-trending.html' },
    { k: 'mcp', label: 'MCP 目录', href: 'mcp.html' },
    { k: 'about', label: '关于', href: 'about.html' },
  ];
  const active = document.body.dataset.nav;
  const iconBtn = 'grid place-items-center w-10 h-10 rounded-lg text-zinc-600 hover:text-zinc-900 hover:bg-zinc-100 transition-colors';

  const header = document.getElementById('site-header');
  if (header) {
    header.outerHTML = `
    <a href="#main" class="sr-only focus:not-sr-only focus:fixed focus:top-3 focus:left-3 focus:z-[100] focus:bg-white focus:px-3 focus:py-2 focus:rounded-lg focus:shadow">跳到正文</a>
    <header class="sticky top-0 z-40 bg-white/80 backdrop-blur-md border-b border-zinc-200/80">
      <div class="max-w-site mx-auto px-4 md:px-6 h-16 flex items-center gap-4">
        <a href="home.html" class="flex items-center gap-2.5 shrink-0 rounded-lg" aria-label="Silva 首页">
          <span class="grid place-items-center w-8 h-8 rounded-[10px] bg-zinc-900 text-white font-display font-bold text-[15px]">S</span>
          <span class="font-display font-bold text-[18px] tracking-tight text-zinc-900">Silva</span>
        </a>
        <nav aria-label="主导航" class="hidden md:block ml-auto">
          <ul class="flex items-center gap-0.5 text-[14px]">
            ${NAV.map(n => `<li><a href="${n.href}" class="block px-3 py-2 rounded-lg transition-colors ${n.k === active ? 'bg-zinc-100 text-zinc-900 font-medium' : 'text-zinc-600 hover:text-zinc-900 hover:bg-zinc-50'}" ${n.k === active ? 'aria-current="page"' : ''}>${n.label}</a></li>`).join('')}
          </ul>
        </nav>
        <div class="hidden md:flex items-center gap-1 pl-3 ml-1 border-l border-zinc-200">
          <a href="#rss" class="${iconBtn}" aria-label="RSS 订阅">${icon('rss')}</a>
          <a href="https://github.com/" target="_blank" rel="noopener" class="${iconBtn}" aria-label="GitHub（新窗口打开）">${icon('github')}</a>
        </div>
        <button type="button" class="md:hidden ml-auto grid place-items-center w-11 h-11 -mr-2 rounded-lg text-zinc-800 hover:bg-zinc-100" data-toggle="mobile-nav" aria-expanded="false" aria-controls="mobile-nav" aria-label="打开菜单">${icon('menu', 'w-5 h-5')}</button>
      </div>
      <nav id="mobile-nav" hidden aria-label="主导航（手机）" class="md:hidden border-t border-zinc-200 bg-white">
        <ul class="px-3 py-3 grid gap-1">
          ${NAV.map(n => `<li><a href="${n.href}" class="flex items-center justify-between h-12 px-3 rounded-xl text-[16px] ${n.k === active ? 'bg-zinc-100 text-zinc-900 font-medium' : 'text-zinc-700 hover:bg-zinc-50'}" ${n.k === active ? 'aria-current="page"' : ''}>${n.label}${icon('chevron-right', 'w-4 h-4 text-zinc-400')}</a></li>`).join('')}
        </ul>
        <div class="flex gap-2 px-6 pb-4">
          <a href="#rss" class="btn btn-ghost flex-1">${icon('rss')}RSS</a>
          <a href="https://github.com/" target="_blank" rel="noopener" class="btn btn-ghost flex-1">${icon('github')}GitHub</a>
        </div>
      </nav>
    </header>`;
  }

  const footer = document.getElementById('site-footer');
  if (footer) {
    const col = (title, items) => `<div><h2 class="text-[13px] font-medium text-zinc-900">${title}</h2><ul class="mt-3 space-y-2.5 text-[14px]">${items.map(([l, h]) => `<li><a class="text-zinc-600 hover:text-zinc-900" href="${h}">${l}</a></li>`).join('')}</ul></div>`;
    const fbtn = 'grid place-items-center w-10 h-10 rounded-lg border border-zinc-200 text-zinc-600 hover:text-zinc-900 hover:border-zinc-300 transition-colors';
    footer.outerHTML = `
    <footer class="mt-20 md:mt-28 border-t border-zinc-200 bg-white">
      <div class="max-w-site mx-auto px-4 md:px-6 py-12 grid gap-10 md:grid-cols-12">
        <div class="md:col-span-5">
          <a href="home.html" class="inline-flex items-center gap-2.5">
            <span class="grid place-items-center w-8 h-8 rounded-[10px] bg-zinc-900 text-white font-display font-bold text-[15px]">S</span>
            <span class="font-display font-bold text-[18px] tracking-tight">Silva</span>
          </a>
          <p class="mt-4 text-[14px] leading-7 text-zinc-600 max-w-sm">周屿的个人技术主页：后端与工程实践文章、做过的项目、常用工具，以及 AI 开源项目趋势与 MCP Server 目录。</p>
          <div class="mt-5 flex gap-2">
            <a href="https://github.com/" target="_blank" rel="noopener" class="${fbtn}" aria-label="GitHub（新窗口打开）">${icon('github')}</a>
            <a href="mailto:hello@example.com" class="${fbtn}" aria-label="发邮件给周屿">${icon('mail')}</a>
            <a href="#rss" class="${fbtn}" aria-label="RSS 订阅">${icon('rss')}</a>
          </div>
        </div>
        <nav aria-label="页脚栏目" class="md:col-span-7 grid grid-cols-3 gap-6">
          ${col('内容', [['文章', 'blog.html'], ['项目', 'projects.html'], ['工具', 'tools.html']])}
          ${col('数据', [['AI 趋势', 'ai-trending.html'], ['MCP 目录', 'mcp.html']])}
          ${col('站点', [['关于', 'about.html'], ['RSS 订阅', '#rss']])}
        </nav>
      </div>
      <div class="border-t border-zinc-100">
        <div class="max-w-site mx-auto px-4 md:px-6 py-5 text-[12px] text-zinc-500 flex flex-wrap justify-between gap-2">
          <span>© 2026 周屿</span>
          <span class="font-mono">site g-0142 · snapshot 2026-11-20</span>
        </div>
      </div>
    </footer>`;
  }

  hydrateIcons();
})();
