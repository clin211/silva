export type Metric = 'stars' | 'forks';
export type WindowDays = 7 | 30;

export interface Sample {
    date: string;
    stars: number;
    forks: number;
}

export interface Repository {
    slug: string;
    name: string;
    description: string;
    language: string;
    topics: string[];
    category: string;
    stars: number;
    forks: number;
    updated: string;
    featured: boolean;
    growth: Record<Metric, Record<WindowDays, number | null>>;
}

type Seed = [
    slug: string,
    name: string,
    description: string,
    language: string,
    topics: string[],
    category: string,
    stars: number,
    forks: number,
    growthPerDay: number,
];

const seeds: Seed[] = [
    [
        'langgraph',
        'langchain-ai/langgraph',
        '构建可恢复的有状态智能体工作流。',
        'Python',
        ['agents', 'llm'],
        '智能体',
        27240,
        4380,
        146,
    ],
    [
        'autogen',
        'microsoft/autogen',
        '通过多个智能体协作执行复杂任务。',
        'Python',
        ['agents', 'llm'],
        '智能体',
        46395,
        7560,
        32,
    ],
    [
        'vllm',
        'vllm-project/vllm',
        '面向大模型的高吞吐推理与服务引擎。',
        'Python',
        ['llm', 'llm-inference'],
        '推理与部署',
        65890,
        10480,
        178,
    ],
    [
        'llama-cpp',
        'ggml-org/llama.cpp',
        '在本地设备上运行量化语言模型。',
        'C++',
        ['llm', 'llm-inference'],
        '推理与部署',
        91870,
        13940,
        66,
    ],
    [
        'lancedb',
        'lancedb/lancedb',
        '针对多模态数据的嵌入式向量数据库。',
        'Rust',
        ['rag', 'vector-database'],
        '检索与数据',
        8620,
        745,
        43,
    ],
    [
        'dify',
        'langgenius/dify',
        '组合模型、检索与工具以开发 AI 应用。',
        'TypeScript',
        ['agents', 'rag'],
        '智能体',
        108120,
        16420,
        184,
    ],
    [
        'qdrant',
        'qdrant/qdrant',
        '将向量检索与载荷过滤放入同一个服务。',
        'Rust',
        ['rag', 'vector-database'],
        '检索与数据',
        25900,
        1730,
        31,
    ],
    [
        'graphrag',
        'microsoft/graphrag',
        '用知识图谱组织文本集合的检索和推理。',
        'Python',
        ['rag', 'llm'],
        '检索与数据',
        24760,
        2450,
        8,
    ],
    [
        'continue',
        'continuedev/continue',
        '为编辑器提供可配置的开源代码助手。',
        'TypeScript',
        ['code-agent', 'agents'],
        '开发工具',
        31250,
        3225,
        38,
    ],
    [
        'aider',
        'Aider-AI/aider',
        '在终端与模型协作编辑代码。',
        'Python',
        ['code-agent', 'llm'],
        '开发工具',
        36810,
        3180,
        71,
    ],
    [
        'open-webui',
        'open-webui/open-webui',
        '面向本地模型的自托管对话界面。',
        'Python',
        ['llm', 'rag'],
        '开发工具',
        97200,
        12800,
        86,
    ],
    [
        'llamafactory',
        'hiyouga/LlamaFactory',
        '在一套流程中微调不同的开源模型。',
        'Python',
        ['fine-tuning', 'llm'],
        '模型训练',
        54430,
        6720,
        53,
    ],
    [
        'axolotl',
        'axolotl-ai-cloud/axolotl',
        '以配置文件管理语言模型的微调任务。',
        'Python',
        ['fine-tuning', 'llm'],
        '模型训练',
        14820,
        1580,
        -2,
    ],
    [
        'unsloth',
        'unslothai/unsloth',
        '面向有限显存的模型训练工具。',
        'Python',
        ['fine-tuning', 'llm'],
        '模型训练',
        38120,
        3024,
        116,
    ],
    [
        'ollama',
        'ollama/ollama',
        '在本地下载、运行和管理语言模型。',
        'Go',
        ['llm', 'llm-inference'],
        '推理与部署',
        132930,
        10965,
        103,
    ],
    [
        'sglang',
        'sgl-project/sglang',
        '用于大语言模型与视觉模型的推理服务。',
        'Python',
        ['llm-inference', 'llm'],
        '推理与部署',
        17840,
        2890,
        150,
    ],
    [
        'litellm',
        'BerriAI/litellm',
        '用统一的调用格式接入多个模型提供方。',
        'Python',
        ['llm', 'llm-inference'],
        '推理与部署',
        29440,
        4510,
        74,
    ],
    [
        'ragflow',
        'infiniflow/ragflow',
        '从文档解析到检索问答的工作流。',
        'Python',
        ['rag', 'agents'],
        '检索与数据',
        62470,
        6380,
        128,
    ],
    [
        'flowise',
        'FlowiseAI/Flowise',
        '以可视化方式构建检索与智能体流程。',
        'TypeScript',
        ['agents', 'rag'],
        '智能体',
        43150,
        2060,
        27,
    ],
    [
        'chroma',
        'chroma-core/chroma',
        '为检索应用存储嵌入向量和元数据。',
        'Rust',
        ['vector-database', 'rag'],
        '检索与数据',
        19380,
        1620,
        19,
    ],
    [
        'mlx',
        'ml-explore/mlx',
        '面向 Apple 芯片的机器学习数组框架。',
        'C++',
        ['fine-tuning', 'llm-inference'],
        '模型训练',
        21380,
        1380,
        23,
    ],
    [
        'mastra',
        'mastra-ai/mastra',
        '在 TypeScript 中组合智能体、工具和评估。',
        'TypeScript',
        ['agents', 'rag'],
        '智能体',
        17430,
        1180,
        49,
    ],
    [
        'ktransformers',
        'kvcache-ai/ktransformers',
        '在异构硬件上运行大模型的推理工具。',
        'Python',
        ['llm-inference', 'llm'],
        '推理与部署',
        14830,
        1440,
        22,
    ],
    [
        'promptfoo',
        'promptfoo/promptfoo',
        '为模型和提示词创建可重复的回归评估。',
        'TypeScript',
        ['llm', 'agents'],
        '开发工具',
        9130,
        710,
        25,
    ],
];

// The bundled fixture stores illustrative counts for public GitHub projects.
function buildFixture(date: string | null) {
    if (date === null) return null;

    const epoch = Date.parse(date);
    const day = 86_400_000;
    const dateAt = (offset: number) => new Date(epoch - offset * day).toISOString().slice(0, 10);
    const history = new Map<string, Sample[]>();

    const repos: Repository[] = seeds.map((seed, index) => {
        const [slug, name, description, language, topics, category, stars, forks, slope] = seed;
        const sampleCount = slug === 'lancedb' ? 1 : slug === 'promptfoo' ? 12 : 60;
        const forkSlope = Math.sign(slope) * Math.ceil(Math.abs(slope) / 12);
        const samples = Array.from({ length: sampleCount }, (_, i) => {
            const offset = sampleCount - i - 1;
            return {
                date: dateAt(offset),
                stars:
                    stars -
                    offset * slope +
                    Math.round(Math.sin(offset / 2.5) * Math.min(28, Math.abs(slope))),
                forks:
                    forks -
                    offset * forkSlope +
                    Math.round(Math.sin(offset / 3) * Math.min(6, Math.abs(forkSlope))),
            };
        });
        history.set(slug, samples);

        function netChange(metric: Metric, window: WindowDays) {
            const previous = samples.at(-window - 1);
            return previous ? samples[samples.length - 1][metric] - previous[metric] : null;
        }

        return {
            slug,
            name,
            description,
            language,
            topics,
            category,
            stars,
            forks,
            updated: dateAt(index % 12),
            featured: ['langgraph', 'vllm', 'lancedb'].includes(slug),
            growth: {
                stars: { 7: netChange('stars', 7), 30: netChange('stars', 30) },
                forks: { 7: netChange('forks', 7), 30: netChange('forks', 30) },
            },
        };
    });

    return { date, repos, history };
}

const fixture = buildFixture('2026-11-20');

interface TrendingSnapshot {
    date: string;
    repos: Repository[];
}

export function loadTrending(): TrendingSnapshot | null {
    return fixture && { date: fixture.date, repos: fixture.repos };
}

export function historyFor(slug: string) {
    return fixture?.history.get(slug) ?? [];
}
