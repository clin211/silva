import { useEffect, useRef, useState } from 'react';
import type { Metric, Sample } from '../lib/trending';
import { number } from '../lib/format';

const metrics = [
    ['stars', 'Stars'],
    ['forks', 'Forks'],
] as const;

function drawSeries(canvas: HTMLCanvasElement, samples: Sample[], metric: Metric) {
    const { width, height } = canvas.getBoundingClientRect();
    if (!width || !height) return;
    const pixelRatio = Math.min(window.devicePixelRatio || 1, 3);
    canvas.width = Math.round(width * pixelRatio);
    canvas.height = Math.round(height * pixelRatio);
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.scale(pixelRatio, pixelRatio);

    const values = samples.map((sample) => sample[metric]);
    const min = Math.min(...values);
    const max = Math.max(...values);
    const padding = Math.max(1, (max - min) * 0.08);
    const floor = Math.max(0, Math.floor(min - padding));
    const ceiling = Math.ceil(max + padding);
    const start = 64;
    const end = width - 20;
    const top = 18;
    const bottom = height - 32;
    const firstDay = Date.parse(samples[0].date);
    const span = Date.parse(samples[samples.length - 1].date) - firstDay;
    const x = (sample: Sample) =>
        start + ((Date.parse(sample.date) - firstDay) / span) * (end - start);
    const y = (value: number) => bottom - ((value - floor) / (ceiling - floor)) * (bottom - top);

    ctx.fillStyle = '#5B6F6C';
    ctx.font = '12px system-ui, sans-serif';
    ctx.textAlign = 'right';
    ctx.textBaseline = 'middle';
    for (let index = 0; index < 5; index++) {
        const value = floor + (index * (ceiling - floor)) / 4;
        const line = y(value);
        ctx.strokeStyle = '#e2ebe8';
        ctx.beginPath();
        ctx.moveTo(start, line);
        ctx.lineTo(end, line);
        ctx.stroke();
        ctx.fillText(number(Math.round(value)), start - 8, line);
    }

    const drawLine = () => {
        samples.forEach((sample, index) => {
            if (index === 0) ctx.moveTo(x(sample), y(sample[metric]));
            else ctx.lineTo(x(sample), y(sample[metric]));
        });
    };
    ctx.beginPath();
    drawLine();
    ctx.lineTo(end, bottom);
    ctx.lineTo(start, bottom);
    ctx.closePath();
    const shade = ctx.createLinearGradient(0, top, 0, bottom);
    shade.addColorStop(0, 'rgba(11,137,146,.22)');
    shade.addColorStop(1, 'rgba(11,137,146,.02)');
    ctx.fillStyle = shade;
    ctx.fill();

    ctx.beginPath();
    drawLine();
    ctx.strokeStyle = '#087F89';
    ctx.lineWidth = 2.5;
    ctx.lineJoin = 'round';
    ctx.stroke();

    ctx.fillStyle = '#5B6F6C';
    ctx.textAlign = 'left';
    ctx.fillText(samples[0].date.slice(5), start, bottom + 18);
    ctx.textAlign = 'right';
    ctx.fillText(samples[samples.length - 1].date.slice(5), end, bottom + 18);
}

export default function TrendChart({ samples }: { samples: Sample[] }) {
    const [metric, setMetric] = useState<Metric>('stars');
    const canvas = useRef<HTMLCanvasElement>(null);
    useEffect(() => {
        const element = canvas.current;
        if (!element || samples.length < 2) return;
        const draw = () => drawSeries(element, samples, metric);
        const observer = new ResizeObserver(draw);
        observer.observe(element);
        draw();
        return () => observer.disconnect();
    }, [metric, samples]);

    return (
        <figure className="panel trend-chart">
            <figcaption className="section-heading">
                <span>每日快照</span>
                <fieldset className="choice">
                    <legend className="sr-only">趋势图指标</legend>
                    {metrics.map(([key, label]) => (
                        <button
                            key={key}
                            type="button"
                            aria-pressed={key === metric}
                            onClick={() => setMetric(key)}
                        >
                            {label}
                        </button>
                    ))}
                </fieldset>
            </figcaption>
            <canvas ref={canvas} aria-hidden="true" />
            <noscript>
                <p className="t-small muted">每日指标见下方快照表。</p>
            </noscript>
        </figure>
    );
}
