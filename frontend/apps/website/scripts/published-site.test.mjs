import assert from 'node:assert/strict';
import { once } from 'node:events';
import { readFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import test from 'node:test';

async function serve(t, handler) {
    const server = createServer(handler);
    server.listen(0, '127.0.0.1');
    await once(server, 'listening');
    t.after(() => {
        server.closeAllConnections();
        return new Promise((resolve) => server.close(resolve));
    });
    return `http://127.0.0.1:${server.address().port}`;
}

test('同一 SSR 进程反映发布、更新、撤回，校验失败时返回 503', async (t) => {
    const snapshot = JSON.parse(
        await readFile(new URL('../fixtures/snapshot.sample.json', import.meta.url), 'utf8'),
    );
    const conditional = [];
    let unavailable = false;
    process.env.SILVA_SITE_SNAPSHOT_PATH = '/internal/site-snapshot';
    process.env.SILVA_PUBLIC_API_ORIGIN = await serve(t, (req, res) => {
        if (req.url === '/api/v1/public/media/preview') {
            return res.writeHead(200, { 'Content-Type': 'image/webp' }).end('preview image');
        }
        if (req.url !== '/internal/site-snapshot')
            return res.writeHead(404).end('Unexpected upstream path ' + req.url);
        const tag = `"site-${snapshot.generation}"`;
        conditional.push(req.headers['if-none-match']);
        if (unavailable) return res.writeHead(503).end();
        if (req.headers['if-none-match'] === tag) return res.writeHead(304, { ETag: tag }).end();
        return res
            .writeHead(200, { ETag: tag, 'Content-Type': 'application/json' })
            .end(JSON.stringify({ data: snapshot }));
    });
    process.env.ASTRO_NODE_AUTOSTART = 'disabled';
    process.env.SITE_URL = 'https://silva.example';
    const { handler } = await import('../dist/server/entry.mjs');
    const origin = await serve(t, handler);
    async function read(path, status = 200) {
        const response = await fetch(origin + path, { signal: AbortSignal.timeout(5000) });
        const body = await response.text();
        assert.equal(response.status, status, path);
        assert.match(response.headers.get('cache-control') || '', /no-store/);
        if (status === 200)
            assert.equal(response.headers.get('x-site-generation'), String(snapshot.generation));
        return body;
    }

    const image = await fetch(origin + '/api/v1/public/media/preview', {
        signal: AbortSignal.timeout(5000),
    });
    assert.equal(image.status, 200, image.url);
    assert.equal(image.headers.get('content-type'), 'image/webp');
    assert.equal(await image.text(), 'preview image');
    const post = snapshot.posts[0];
    const path = `/blog/${post.slug}/`;
    const detail = await read(path);
    assert.ok(detail.includes(post.title));
    assert.ok(detail.includes(`href="https://silva.example${path}"`));
    assert.ok((await read('/rss.xml')).includes(post.slug));
    assert.ok((await read('/sitemap-index.xml')).includes('https://silva.example/sitemap-0.xml'));
    assert.ok((await read('/sitemap-0.xml')).includes(path));
    const home = await read('/');
    assert.ok(home.includes(post.title));
    assert.ok(home.includes('按领域探索'));
    const desktopNavMatch = home.match(/<nav class="nav-desktop"[\s\S]*?<\/nav>/);
    assert.ok(desktopNavMatch, 'desktop nav should be rendered');
    assert.ok(
        desktopNavMatch[0].includes('href="/series/"') && desktopNavMatch[0].includes('博客'),
        'desktop nav should expose 博客 pointing to /series/',
    );
    assert.ok(
        !desktopNavMatch[0].includes('href="/blog/"'),
        'desktop nav should not include 文章 (/blog/) menu item',
    );
    const seriesIndex = await read('/series/');
    assert.ok(seriesIndex.includes('data-category-group="backend"'));
    assert.ok(seriesIndex.includes('data-category-group="rust"'));
    assert.ok(seriesIndex.includes('data-domain-switcher'));
    assert.ok(seriesIndex.includes('data-domain-panel="backend"'));
    assert.ok(seriesIndex.includes('data-domain-panel="rust"'));
    assert.ok(seriesIndex.includes('跨领域公用延伸'));
    assert.ok(seriesIndex.includes('data-series-search'));
    assert.match(seriesIndex, /<a class="nav-link" href="\/series\/" aria-current="page">\s*博客/);
    const rustDomainPage = await read('/series/?domain=rust');
    assert.ok(rustDomainPage.includes('data-domain-panel="rust"'));
    const seriesDetail = await read('/series/' + snapshot.series[0].slug + '/');
    assert.match(seriesDetail, /<a class="nav-link" href="\/series\/" aria-current="true">\s*博客/);
    assert.match(detail, /<a class="nav-link" href="\/series\/" aria-current="true">\s*博客/);
    const categoryPage = await read('/categories/' + post.categories[0] + '/');
    assert.match(categoryPage, /<a class="nav-link" href="\/series\/" aria-current="true">\s*博客/);
    const tagPage = await read('/tags/' + post.tags[0] + '/');
    assert.match(tagPage, /<a class="nav-link" href="\/series\/" aria-current="true">\s*博客/);
    assert.ok(conditional.includes(`"site-${snapshot.generation}"`));

    snapshot.generation++;
    post.title = '修订后的公开标题';
    assert.ok((await read(path)).includes(post.title));
    snapshot.generation++;
    const seriesPath = '/series/' + snapshot.series[0].slug + '/';
    snapshot.posts = [];
    snapshot.series = [];
    assert.ok((await read(path, 404)).includes('页面不存在'));
    await read(seriesPath, 404);
    await read('/categories/' + post.categories[0] + '/', 404);
    await read('/tags/' + post.tags[0] + '/', 404);
    for (const route of ['/', '/blog/', '/series/', '/rss.xml', '/sitemap-0.xml']) {
        assert.ok(!(await read(route)).includes(path), route);
    }
    await read('/never-published/', 404);
    unavailable = true;
    await read('/', 503);
});
