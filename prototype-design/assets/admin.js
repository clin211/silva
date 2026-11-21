/*
 * admin.js — admin shell: grouped sidebar (permission aware), breadcrumb top bar, account menu.
 * Usage:
 *   <body class="bg-ui-bg" data-admin-nav="posts" data-crumbs="内容|文章@posts.html|编辑" data-proto-roles="editor">
 *     <main id="admin-main"> ...page... </main>
 *   Load admin.js, then proto.js.
 * Nav items carry data-role so proto.js role switcher hides what the role can't access.
 */
(function () {
  const GROUPS = [
    { label: null, items: [{ k: 'dashboard', label: '工作台', href: 'dashboard.html', icon: 'M4 13h6V4H4zm10 7h6v-9h-6zM4 20h6v-4H4zm10-16v4h6V4z' }] },
    { label: '内容', items: [
      { k: 'posts', label: '文章', href: 'posts.html', icon: 'M6 3h9l4 4v14H6zM9 12h7M9 16h7M9 8h4' },
      { k: 'taxonomy', label: '分类与标签', href: 'taxonomy.html', role: 'editor', icon: 'M3 12l9-9h8v8l-9 9zM15.5 8.5h.01' },
      { k: 'media', label: '媒体库', href: 'media.html', icon: 'M4 5h16v14H4zM4 16l5-5 4 4 3-3 4 4M15 9h.01' },
    ] },
    { label: '资源', role: 'editor', items: [
      { k: 'projects', label: '项目', href: 'projects.html', role: 'editor', icon: 'M3 7h18v13H3zM8 7V4h8v3' },
      { k: 'tools', label: '工具', href: 'tools.html', role: 'editor', icon: 'M14 6l4 4-9 9H5v-4zM12 8l4 4' },
      { k: 'profile', label: '个人资料', href: 'profile.html', role: 'editor', icon: 'M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM4 21a8 8 0 0 1 16 0' },
      { k: 'mcp', label: 'MCP 目录', href: 'mcp.html', role: 'editor', icon: 'M7 7h10v10H7zM12 2v5M12 17v5M2 12h5M17 12h5' },
      { k: 'mcp-categories', label: 'MCP 分类', href: 'mcp-categories.html', role: 'editor', sub: true },
    ] },
    { label: '运行', items: [
      { k: 'ai-trending', label: 'AI 趋势', href: 'ai-trending.html', role: 'editor', icon: 'M3 17l6-6 4 4 8-8M15 7h6v6' },
      { k: 'ai-runs', label: '采集批次', href: 'ai-runs.html', role: 'editor', sub: true },
      { k: 'publishing', label: '站点发布', href: 'publishing.html', icon: 'M12 3v12M7 8l5-5 5 5M4 15v5h16v-5' },
    ] },
    { label: '系统', role: 'admin', items: [
      { k: 'users', label: '账户', href: 'users.html', role: 'admin', icon: 'M9 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM2 21a7 7 0 0 1 14 0M17 11a3 3 0 1 0 0-6M22 21a6 6 0 0 0-4-5.6' },
      { k: 'roles', label: '角色', href: 'roles.html', role: 'admin', sub: true },
      { k: 'audit-logs', label: '审计日志', href: 'audit-logs.html', role: 'admin', icon: 'M5 4h14v16H5zM9 8h6M9 12h6M9 16h3' },
      { k: 'system-health', label: '系统健康', href: 'system-health.html', role: 'admin', icon: 'M3 12h4l3-7 4 14 3-7h4' },
    ] },
  ];
  const body = document.body;
  const active = body.dataset.adminNav;
  const main = document.getElementById('admin-main');
  if (!main) return;

  const ic = d => d ? `<svg class="shrink-0" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${d}"/></svg>` : '<span class="w-[18px] shrink-0"></span>';

  const navHTML = GROUPS.map(g => `
    <div ${g.role ? `data-role="${g.role}"` : ''} class="mt-4 first:mt-0">
      ${g.label ? `<div class="px-3 mb-1 text-[12px] text-ui-sub">${g.label}</div>` : ''}
      <ul class="space-y-0.5">
        ${g.items.map(i => `<li ${i.role ? `data-role="${i.role}"` : ''}>
          <a href="${i.href}" class="flex items-center gap-2.5 rounded-md px-3 ${i.sub ? 'py-1.5 text-[13px]' : 'py-2 text-[14px]'} ${i.k === active ? 'bg-accent-tint text-accent font-medium' : 'text-ui-text hover:bg-[#ECEEF2]'}" ${i.k === active ? 'aria-current="page"' : ''}>
            ${i.sub ? '<span class="w-[18px] shrink-0"></span>' : ic(i.icon)}<span>${i.label}</span>
          </a></li>`).join('')}
      </ul>
    </div>`).join('');

  const crumbs = (body.dataset.crumbs || '').split('|').filter(Boolean).map(c => {
    const [label, href] = c.split('@');
    return { label, href };
  });
  const crumbHTML = `<nav aria-label="面包屑" class="min-w-0"><ol class="flex items-center gap-1.5 text-[13px] text-ui-sub truncate">
    ${crumbs.map((c, i) => `<li class="flex items-center gap-1.5 min-w-0">${i ? '<span aria-hidden="true" class="text-[#B3B8C2]">/</span>' : ''}${c.href ? `<a class="hover:text-ui-text truncate" href="${c.href}">${c.label}</a>` : `<span class="${i === crumbs.length - 1 ? 'text-ui-text' : ''} truncate" ${i === crumbs.length - 1 ? 'aria-current="page"' : ''}>${c.label}</span>`}</li>`).join('')}
  </ol></nav>`;

  const brand = `<a href="dashboard.html" class="flex items-center gap-2 px-3 h-14 shrink-0">
      <span class="grid place-items-center w-7 h-7 rounded-md bg-accent text-white font-serif font-bold text-[15px]">S</span>
      <span class="font-semibold text-[15px] text-ui-text">Silva 后台</span></a>`;

  const shell = document.createElement('div');
  shell.className = 'min-h-screen lg:grid lg:grid-cols-[224px_1fr]';
  shell.innerHTML = `
    <a href="#admin-main" class="sr-only focus:not-sr-only focus:fixed focus:top-2 focus:left-2 focus:z-[100] focus:bg-white focus:px-3 focus:py-2">跳到工作区</a>
    <aside class="hidden lg:flex flex-col border-r border-ui-line bg-[#FAFAFB] sticky top-0 h-screen" aria-label="后台导航">
      ${brand}
      <nav class="flex-1 overflow-y-auto px-2 py-3">${navHTML}</nav>
      <div class="px-3 py-3 border-t border-ui-line text-[12px] text-ui-sub">线上版本 <span class="tnum text-ui-text">g-0142</span></div>
    </aside>
    <div id="nav-drawer" data-overlay hidden class="fixed inset-0 z-50 lg:hidden">
      <div class="absolute inset-0 drawer-backdrop" data-overlay-backdrop></div>
      <aside class="relative w-[264px] max-w-[85vw] h-full bg-[#FAFAFB] flex flex-col shadow-xl" aria-label="后台导航">
        <div class="flex items-center justify-between pr-2">${brand}<button type="button" data-close class="p-2 text-ui-sub" aria-label="关闭导航">✕</button></div>
        <nav class="flex-1 overflow-y-auto px-2 py-3">${navHTML}</nav>
      </aside>
    </div>
    <div class="min-w-0 flex flex-col">
      <header class="sticky top-0 z-30 h-14 bg-white/95 backdrop-blur border-b border-ui-line flex items-center gap-3 px-4 md:px-6">
        <button type="button" class="lg:hidden -ml-1 p-1.5 rounded text-ui-text hover:bg-ui-bg" data-open="nav-drawer" aria-label="打开导航">
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M4 7h16M4 12h16M4 17h16"/></svg>
        </button>
        <div class="flex-1 min-w-0">${crumbHTML}</div>
        <a href="../site/home.html" target="_blank" rel="noopener" class="hidden sm:inline text-[13px] text-ui-sub hover:text-ui-text">查看站点 ↗</a>
        <div class="relative">
          <button type="button" data-toggle="account-menu" aria-expanded="false" aria-controls="account-menu" class="flex items-center gap-2 rounded-md pl-1 pr-2 py-1 hover:bg-ui-bg">
            <span class="grid place-items-center w-7 h-7 rounded-full bg-[#DCE3EE] text-accent text-[12px] font-semibold">周</span>
            <span class="hidden sm:flex flex-col items-start leading-tight"><span class="text-[13px] text-ui-text">周屿</span><span class="text-[11px] text-ui-sub" data-current-role>Admin</span></span>
          </button>
          <div id="account-menu" hidden class="absolute right-0 mt-2 w-52 rounded-lg border border-ui-line bg-white shadow-lg py-1 text-[14px]">
            <div class="px-3 py-2 border-b border-ui-line"><div class="text-ui-text">周屿</div><div class="text-[12px] text-ui-sub">zhouyu@silva.dev · <span data-current-role>Admin</span></div></div>
            <a href="profile.html" data-role="editor" class="block px-3 py-2 hover:bg-ui-bg">个人资料</a>
            <a href="login.html" class="block px-3 py-2 hover:bg-ui-bg text-bad">退出登录</a>
          </div>
        </div>
      </header>
    </div>`;
  main.parentNode.insertBefore(shell, main);
  shell.lastElementChild.appendChild(main);
  main.classList.add('flex-1', 'min-w-0', 'px-4', 'md:px-6', 'py-6', 'pb-24');
})();
