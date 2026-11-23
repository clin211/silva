---
title: 用 Astro 构建以内容为主的站点
summary: 让内容、分类和订阅源在同一次构建里保持一致。
published: '2026-10-21'
category: frontend
tags: [astro, typescript]
cover:
    color: sun
    mark: SSG
---

技术博客的主要路径是打开列表、阅读文章和订阅更新。先让这些路径有完整的静态 HTML，再逐块处理需要浏览器脚本的交互。

## 使用同一份内容

文章详情读取的标题和发布时间也用于首页、归档和 RSS。静态生成动态路由时，内容列表同时决定哪些 URL 存在，便于检查漏链。

## 按需添加交互

用浏览器脚本补齐主题切换、目录高亮或榜单筛选。测试时关闭 JavaScript，确认文章正文和导航仍然可用。
