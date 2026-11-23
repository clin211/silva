import { defineCollection } from 'astro:content';
import { glob } from 'astro/loaders';
import { z } from 'astro/zod';
import { categorySlugs, tagSlugs } from './data/taxonomy';

const posts = defineCollection({
    loader: glob({ base: './src/content/posts', pattern: '*.md' }),
    schema: z
        .object({
            title: z.string(),
            summary: z.string(),
            published: z.iso.date(),
            updated: z.iso.date().optional(),
            category: z.enum(categorySlugs),
            tags: z.array(z.enum(tagSlugs)),
            featured: z.boolean().default(false),
            cover: z
                .object({
                    color: z.enum(['teal', 'deep', 'sun', 'coral', 'mist', 'ink']),
                    mark: z.string(),
                })
                .optional(),
        })
        .refine(({ published, updated }) => !updated || updated >= published, {
            message: '更新时间须晚于或等于发布时间',
        }),
});

const profile = defineCollection({
    loader: glob({ base: './src/content/profile', pattern: '*.md' }),
    schema: z.object({
        name: z.string(),
        initials: z.string(),
        location: z.string().optional(),
        title: z.string(),
        summary: z.string(),
        focus: z.array(z.string()).default([]),
        links: z
            .array(z.object({ label: z.string(), url: z.url({ protocol: /^https?$/ }) }))
            .default([]),
        experience: z
            .array(
                z.object({
                    period: z.string().optional(),
                    role: z.string(),
                    organization: z.string().optional(),
                    description: z.string(),
                }),
            )
            .default([]),
        skills: z
            .array(
                z.object({
                    group: z.string(),
                    items: z.array(z.string()),
                }),
            )
            .default([]),
    }),
});

export const collections = { posts, profile };
