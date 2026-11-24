// 用示例快照在临时目录中执行一次快照模式构建，便于本地验证快照契约。
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const snapshot =
    process.env.SILVA_SNAPSHOT || join(import.meta.dirname, '../fixtures/snapshot.sample.json');
const bytes = readFileSync(snapshot);
// Astro 以 rename 移动构建资源，输出目录须与站点目录在同一文件系统
const root = resolve(import.meta.dirname, '..');
const tempRoot = statSync(tmpdir()).dev === statSync(root).dev ? tmpdir() : join(root, '.astro');
const outDir = process.env.SILVA_SITE_OUT_DIR || mkdtempSync(join(tempRoot, 'silva-site-'));
const env = {
    ...process.env,
    SILVA_SNAPSHOT: snapshot,
    SILVA_SNAPSHOT_SHA256:
        process.env.SILVA_SNAPSHOT_SHA256 || createHash('sha256').update(bytes).digest('hex'),
    SILVA_SNAPSHOT_GENERATION:
        process.env.SILVA_SNAPSHOT_GENERATION || String(JSON.parse(bytes.toString()).generation),
    SILVA_SITE_OUT_DIR: outDir,
};

console.log(`快照 ${snapshot}\nSHA-256 ${env.SILVA_SNAPSHOT_SHA256}\n输出 ${outDir}`);
const { status } = spawnSync('astro', ['build'], { env, stdio: 'inherit' });
process.exit(status ?? 1);
