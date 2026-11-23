export const categorySlugs = [
    'backend',
    'frontend',
    'ai-engineering',
    'devops',
    'cross-platform',
    'notes',
] as const;
export const tagSlugs = [
    'go',
    'docker',
    'github-actions',
    'cms',
    'astro',
    'typescript',
    'rag',
    'ai-agent',
    'react-native',
    'javascript',
    'postgresql',
    'learning',
] as const;

export const categories: Record<
    (typeof categorySlugs)[number],
    { name: string; description: string; color?: string }
> = {
    backend: {
        name: '后端',
        description: '服务端架构、数据一致性与工程实践。',
        color: 'chip-teal',
    },
    frontend: { name: '前端', description: 'Web 框架、应用架构与实际交付。' },
    'ai-engineering': {
        name: 'AI 工程',
        description: '从检索到工具调用，记录 AI 应用的实现和评估。',
        color: 'chip-coral',
    },
    devops: { name: '工程化', description: '构建、部署和可重复的开发环境。', color: 'chip-teal' },
    'cross-platform': {
        name: '跨端',
        description: '跨端应用开发、发布与维护的实践。',
        color: 'chip-sun',
    },
    notes: { name: '随笔', description: '写代码之外的学习和回顾。' },
};

export const tags: Record<(typeof tagSlugs)[number], string> = {
    go: 'Go',
    docker: 'Docker',
    'github-actions': 'GitHub Actions',
    cms: 'CMS',
    astro: 'Astro',
    typescript: 'TypeScript',
    rag: 'RAG',
    'ai-agent': 'AI Agent',
    'react-native': 'React Native',
    javascript: 'JavaScript',
    postgresql: 'PostgreSQL',
    learning: '持续学习',
};
